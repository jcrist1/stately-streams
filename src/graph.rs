use std::{marker::PhantomData, pin::Pin};

use frunk::{prelude::HList, HCons, HNil};
use futures::{
    future::ready,
    future::{Join, Ready},
    Future, Stream, stream::Scan,
};
use tokio::sync::mpsc::Receiver;
use tokio_stream::wrappers::ReceiverStream;

use crate::{
    async_coprod::SelectSubscribable,
    async_hlist::{JoinFuture, JoinSubscribable},
    node::{
        ArbitraryFlowStream, AsyncInnerNode, Node, SourceStream, StreamWrapper, UniformFlowStream, Subscription, GetInternalStream, LockInnerNode,
    },
    sender::SenderHList,
    subscriber::Subscribable, hierarchical_state::{True, False, MutRefHList, Lock, Filter},
};

struct Graph<State, Nodes> {
    state: State,
    nodes: Nodes,
}

impl<TailState: HList, Nodes> Graph<TailState, Nodes> {
    fn add_state<HeadState>(self, head: HeadState) -> Graph<HCons<HeadState, TailState>, Nodes> {
        Graph {
            state: HCons {
                head,
                tail: self.state,
            },
            nodes: self.nodes,
        }
    }
}

impl<State, TailNodes: HList> Graph<State, TailNodes> {
    fn add_source_node<Item, HeadNode: Stream<Item = Item>>(
        self,
        head: HeadNode,
    ) -> Graph<State, HCons<Node<Item, SourceStream<HeadNode>, HNil>, TailNodes>> {
        Graph {
            state: self.state,
            nodes: HCons {
                head: Node::new(SourceStream::new(head)),
                tail: self.nodes,
            },
        }
    }
}

trait UniformFlowStreamHList {}
impl UniformFlowStreamHList for HNil {}
impl<
        OutputType,
        Inner: Stream<Item = OutputType>,
        TailNodes: UniformFlowStreamHList,
    > UniformFlowStreamHList
    for HCons<UniformFlowStream<Inner>, TailNodes>
{
}

impl<
        OutputType,
        Inner: Stream<Item = OutputType>,
        TailNodes: UniformFlowStreamHList,
    > UniformFlowStreamHList
    for HCons<SourceStream<Inner>, TailNodes>
{
}

trait SubscribableHList<Filter> {
    type NewSubscribedNodes: NodeHList;
    type FilteredNodes;
    type Subscriptions: JoinSubscribable + SelectSubscribable;

    fn join_subscribe(self) -> (Self::NewSubscribedNodes, Self::Subscriptions); // todo: dedicated
                                                                                // type
    fn select_subscribe(self) -> (Self::NewSubscribedNodes, Self::Subscriptions); // todo:
                                                                                  // dedicated type
}

impl SubscribableHList<HNil> for HNil {
    type NewSubscribedNodes = HNil;
    type FilteredNodes = HNil;
    type Subscriptions = HNil;
    fn join_subscribe(self) -> (HNil, HNil) {
        (HNil, HNil)
    }

    fn select_subscribe(self) -> (HNil, HNil) {
        (HNil, HNil)
    }
}
impl<
    InnerStream,
    HeadNodeOutput,
    HeadNodeInner, 
    HeadNodeOutputStream, 
    TailNodes, 
    TailFilter
> SubscribableHList<HCons<False, TailFilter>> 
    for 
    HCons<Node<
        HeadNodeOutput,
        HeadNodeInner,
        HeadNodeOutputStream
    >, TailNodes> 
    where TailNodes: SubscribableHList<TailFilter>,
    HeadNodeOutput: Clone + 'static,
    HeadNodeOutputStream: Subscribable<HeadNodeOutput> + SenderHList<HeadNodeOutput>,
    HeadNodeInner: StreamWrapper<Inner = InnerStream> + 'static,
    InnerStream: Stream<Item = HeadNodeOutput> + 'static {
    type NewSubscribedNodes = HCons<Node<
        HeadNodeOutput,
        HeadNodeInner, 
        HeadNodeOutputStream
    >, <TailNodes as SubscribableHList<TailFilter>>::NewSubscribedNodes>;
    type FilteredNodes = <TailNodes as SubscribableHList<TailFilter>>::FilteredNodes;
    type Subscriptions = <TailNodes as SubscribableHList<TailFilter>>::Subscriptions;

    fn join_subscribe(self) -> (Self::NewSubscribedNodes, Self::Subscriptions) {
        let HCons {
            head,
            tail
        } = self;
        let (new_tail, subscriptions) = tail.join_subscribe();
        (HCons {
            head,
            tail: new_tail
        }, subscriptions)
    }

    fn select_subscribe(self) -> (Self::NewSubscribedNodes, Self::Subscriptions) {
        let HCons {
            head,
            tail
        } = self;
        let (new_tail, subscriptions) = tail.select_subscribe();
        (HCons {
            head,
            tail: new_tail
        }, subscriptions)
    }
    

}

impl<
    InnerStream,
    HeadNodeOutput,
    HeadNodeInner, 
    HeadNodeOutputStream, 
    TailNodes, 
    TailFilter
> SubscribableHList<HCons<True, TailFilter>> 
    for 
    HCons<Node<
        HeadNodeOutput,
        HeadNodeInner,
        HeadNodeOutputStream
    >, TailNodes> 
    where TailNodes: SubscribableHList<TailFilter>,
    HeadNodeOutput: Clone + 'static,
    HeadNodeOutputStream: Subscribable<HeadNodeOutput> + SenderHList<HeadNodeOutput>,
    HeadNodeInner: StreamWrapper<Inner = InnerStream> + 'static,
    InnerStream: Stream<Item = HeadNodeOutput> + 'static, 
    <<TailNodes as SubscribableHList<TailFilter>>::Subscriptions as SelectSubscribable>::SubscribedItems: Clone + 'static,
    <<TailNodes as SubscribableHList<TailFilter>>::Subscriptions as JoinSubscribable>::SubscribedItem:  Clone  + 'static,
    <HeadNodeOutputStream as Subscribable<HeadNodeOutput>>::NewSenders: Subscribable<HeadNodeOutput>,
    <HeadNodeOutputStream as Subscribable<HeadNodeOutput>>::NewSenders: SenderHList<HeadNodeOutput>,
{
    type NewSubscribedNodes = HCons<Node<
        HeadNodeOutput,
        HeadNodeInner, 
        HeadNodeOutputStream::NewSenders
    >, <TailNodes as SubscribableHList<TailFilter>>::NewSubscribedNodes>;
    type FilteredNodes = HCons<HeadNodeInner, <TailNodes as SubscribableHList<TailFilter>>::FilteredNodes>;
    type Subscriptions = HCons<ReceiverStream<HeadNodeOutput>, <TailNodes as SubscribableHList<TailFilter>>::Subscriptions>;

    fn join_subscribe(self) -> (Self::NewSubscribedNodes, Self::Subscriptions) {
        let HCons {
            head,
            tail
        } = self;
        let (new_tail, subscriptions) = tail.join_subscribe();
        let Subscription { node, receiver } = 
        head.subscribe();
        (HCons {
            head: node,
            tail: new_tail
        }, HCons { head: ReceiverStream::new(receiver), tail: subscriptions} )
    }

    fn select_subscribe(self) -> (Self::NewSubscribedNodes, Self::Subscriptions) {

        let HCons {
            head,
            tail
        } = self;
        let (new_tail, subscriptions) = tail.select_subscribe();
        let Subscription { node, receiver } = 
        head.subscribe();
        (HCons {
            head: node,
            tail: new_tail
        }, HCons { head: ReceiverStream::new(receiver), tail: subscriptions} )
    }
    

}

impl<State: HList, Nodes: HList> Graph<State, Nodes> {
    fn select_subscribe_with_state<
        NodeFilter: Filter,
        StateFilter: Filter,
        FilteredNodeOutputs: 'static,
        SelectSubscriptionStream: Stream<Item = FilteredNodeOutputs> + 'static,
        FilteredNodesJoinSubscription: SelectSubscribable<SubscriptionStream = SelectSubscriptionStream>,
        FilteredNodes,
        Output: 'static,
        FilteredState: MutRefHList + 'static,
        F,
    >(
        self,
        // we have this to make type inference easier. Don't need to specify all type parameters
        _fltr: NodeFilter,
        _state_flter: StateFilter,
        f: F,
    ) -> Graph<
        State,
        HCons<
            Node<
                Output,
                ArbitraryFlowStream<
                    Scan<
                        SelectSubscriptionStream,
                        (State, F),
                        Ready<Option<Output>>,
                        fn(&'_ mut (State, F), FilteredNodeOutputs) -> Ready<Option<Output>>,
                    >
                >,
                HNil,
            >,
            <Nodes as SubscribableHList<NodeFilter>>::NewSubscribedNodes,
        >,
    >
    where
        Nodes: SubscribableHList<
            NodeFilter,
            FilteredNodes = FilteredNodes,
            Subscriptions = FilteredNodesJoinSubscription,
        >,
        State: Lock<StateFilter, InnerType = FilteredState>,
        F: for<'a> Fn(FilteredState::MutRefHList<'a>, FilteredNodeOutputs) -> Output+ Clone + 'static,
    {
        let Graph { state, nodes } = self;
        let (old_nodes_with_new_subscription, subscription): (
            <Nodes as SubscribableHList<NodeFilter>>::NewSubscribedNodes,
            _,
        ) = SubscribableHList::<NodeFilter>::join_subscribe(nodes);
        let new_node = Node::new(LockInnerNode::new(
            subscription.select_subscribe(),
            state.clone(),
            f,
        ).get_internal_stream().non_uniform());
        let b = new_node;
        let new_nodes = HCons {
            head: b,
            tail: old_nodes_with_new_subscription,
        };
        Graph {
            state,
            nodes: new_nodes,
        }
    }
    fn select_subscribe<
        Filter,
        FilteredNodeOutputs,
        SelectSubscriptionStream: Stream<Item = FilteredNodeOutputs>,
        FilteredNodesJoinSubscription: SelectSubscribable<SubscriptionStream = SelectSubscriptionStream>,
        FilteredNodes,
        Output,
        StreamOutput: Stream<Item = Output>,
        F: FnOnce(SelectSubscriptionStream) -> StreamOutput,
    >(
        self,
        // we have this to make type inference easier. Don't need to specify all type parameters
        _fltr: Filter,
        f: F,
    ) -> Graph<
        State,
        HCons<
            Node<
                Output,
                <AsyncInnerNode<
                    FilteredNodeOutputs,
                    Output,
                    F,
                    SelectSubscriptionStream,
                    StreamOutput,
                > as GetInternalStream>::InternalStream,
                HNil,
            >,
            <Nodes as SubscribableHList<Filter>>::NewSubscribedNodes,
        >,
    >
    where
        Nodes: SubscribableHList<
            Filter,
            FilteredNodes = FilteredNodes,
            Subscriptions = FilteredNodesJoinSubscription,
        >,
    {
        let Graph { state, nodes } = self;
        let (old_nodes_with_new_subscription, subscription): (
            <Nodes as SubscribableHList<Filter>>::NewSubscribedNodes,
            _,
        ) = SubscribableHList::<Filter>::select_subscribe(nodes);
        let new_node = Node::new(AsyncInnerNode::new(
            subscription.select_subscribe(),
            f,
        ).get_internal_stream());
        let b = new_node;
        let new_nodes = HCons {
            head: b,
            tail: old_nodes_with_new_subscription,
        };
        Graph {
            state,
            nodes: new_nodes,
        }
    }
    fn join_subscribe_with_state<
        NodeFilter: Filter,
        StateFilter: Filter,
        FilteredNodeOutputs: 'static,
        JoinSubscriptionStream: Stream<Item = FilteredNodeOutputs> + 'static,
        FilteredNodesJoinSubscription: JoinSubscribable<SubscriptionStream = JoinSubscriptionStream>,
        FilteredNodes: UniformFlowStreamHList,
        Output: 'static,
        FilteredState: MutRefHList + 'static,
        F,
    >(
        self,
        // we have this to make type inference easier. Don't need to specify all type parameters
        _fltr: NodeFilter,
        _state_flter: StateFilter,
        f: F,
    ) -> Graph<
        State,
        HCons<
            Node<
                Output,
                <LockInnerNode<
                    FilteredNodeOutputs,
                    Output,
                    F,
                    State,
                    JoinSubscriptionStream,
                    FilteredState,
        Scan<
            JoinSubscriptionStream,
            (State, F),
            Ready<Option<Output>>,
            fn(&'_ mut (State, F), FilteredNodeOutputs) -> Ready<Option<Output>>,
        >,
                > as GetInternalStream>::InternalStream,
                HNil,
            >,
            <Nodes as SubscribableHList<NodeFilter>>::NewSubscribedNodes,
        >,
    >
    where
        Nodes: SubscribableHList<
            NodeFilter,
            FilteredNodes = FilteredNodes,
            Subscriptions = FilteredNodesJoinSubscription,
        >,
        State: Lock<StateFilter, InnerType = FilteredState>,
        F: for<'a> Fn(FilteredState::MutRefHList<'a>, FilteredNodeOutputs) -> Output+ Clone + 'static,
    {
        let Graph { state, nodes } = self;
        let (old_nodes_with_new_subscription, subscription): (
            <Nodes as SubscribableHList<NodeFilter>>::NewSubscribedNodes,
            _,
        ) = SubscribableHList::<NodeFilter>::join_subscribe(nodes);
        let new_node = Node::new(LockInnerNode::new(
            subscription.join_subscribe(),
            state.clone(),
            f,
        ).get_internal_stream());
        let b = new_node;
        let new_nodes = HCons {
            head: b,
            tail: old_nodes_with_new_subscription,
        };
        Graph {
            state,
            nodes: new_nodes,
        }
    }

    fn join_subscribe<
        Filter,
        FilteredNodeOutputs,
        JoinSubscriptionStream: Stream<Item = FilteredNodeOutputs>,
        FilteredNodesJoinSubscription: JoinSubscribable<SubscriptionStream = JoinSubscriptionStream>,
        FilteredNodes: UniformFlowStreamHList,
        Output,
        StreamOutput: Stream<Item = Output>,
        F: FnOnce(JoinSubscriptionStream) -> StreamOutput,
    >(
        self,
        // we have this to make type inference easier. Don't need to specify all type parameters
        _fltr: Filter,
        f: F,
    ) -> Graph<
        State,
        HCons<
            Node<
                Output,
                <AsyncInnerNode<
                    FilteredNodeOutputs,
                    Output,
                    F,
                    JoinSubscriptionStream,
                    StreamOutput,
                > as GetInternalStream>::InternalStream,
                HNil,
            >,
            <Nodes as SubscribableHList<Filter>>::NewSubscribedNodes,
        >,
    >
    where
        Nodes: SubscribableHList<
            Filter,
            FilteredNodes = FilteredNodes,
            Subscriptions = FilteredNodesJoinSubscription,
        >,
    {
        let Graph { state, nodes } = self;
        let (old_nodes_with_new_subscription, subscription): (
            <Nodes as SubscribableHList<Filter>>::NewSubscribedNodes,
            _,
        ) = SubscribableHList::<Filter>::join_subscribe(nodes);
        let new_node = Node::new(AsyncInnerNode::new(
            subscription.join_subscribe(),
            f,
        ).get_internal_stream());
        let b = new_node;
        let new_nodes = HCons {
            head: b,
            tail: old_nodes_with_new_subscription,
        };
        Graph {
            state,
            nodes: new_nodes,
        }
    }
}

impl<State, Nodes: NodeHList> Graph<State, Nodes> {
    pub fn run(self) -> <Nodes::RunFuture as JoinFuture>::JoinFuture {
        self.nodes.join().join_fut()
    }
}

pub trait NodeHList {
    type RunOut;
    type RunFuture: JoinFuture;
    fn join(self) -> Self::RunFuture;
}

impl NodeHList for HNil {
    type RunOut = HNil;
    type RunFuture = HNil;
    fn join(self) -> HNil {
        HNil
    }
}

impl<OutputType, Inner, InnerStream, TailOutput, TailNodes> NodeHList
    for HCons<Node<OutputType, Inner, TailOutput>, TailNodes>
where
    OutputType: Clone + 'static,
    TailOutput: Subscribable<OutputType> + SenderHList<OutputType>,
    Inner: StreamWrapper<Inner = InnerStream> + 'static,
    InnerStream: Stream<Item = OutputType> + 'static,
    TailNodes: NodeHList,
{
    type RunOut = HCons<usize, TailNodes::RunOut>;
    type RunFuture = HCons<Pin<Box<dyn Future<Output = usize>>>, TailNodes::RunFuture>;
    fn join(self) -> Self::RunFuture {
        let HCons { head, tail } = self;
        HCons {
            head: Box::pin(head.run()),
            tail: tail.join(),
        }
    }
}

struct NodeSubscription<LockFilter, NodeFilter, Node> {
    lock_filter: LockFilter,
    node_filter: NodeFilter,
    node: Node,
}

trait NodeSubscriptionHList {}

#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use std::sync::Arc;

    use frunk::{hlist, HList, HNil, hlist_pat, poly_fn};
    use futures::stream;
    use futures::stream::{StreamExt, Stream};
    use crate::node::StreamWrapper;
    use super::NodeHList;

    use crate::hierarchical_state::False;
    use crate::{util::new_shared, hierarchical_state::True};

    use super::Graph;

    #[derive(Debug, Clone, PartialEq, Hash)]
    struct Person {
        name: String,
        age: usize,
    }
    const ALPHABET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";

    /// In this test we don't actually want to do anything with the locks, we just want to test a
    /// traditional deadlock pattern
    #[tokio::test]
    async fn test_diamond_lock_select() {
        let stuff = new_shared(Vec::<String>::new());
        let people = new_shared(Vec::<Arc<Person>>::new());

        let graph = Graph {
            state: HNil,
            nodes: HNil,
        };

        let int_stream = stream::iter((0..100).into_iter());

        let graph = graph
            .add_state(stuff)
            .add_state(people.clone())
            .add_source_node(int_stream)
            .join_subscribe_with_state(hlist!(True), hlist!(True, False), |hlist_pat!(people), hlist_pat!(int_inp)| {
                int_inp + 1
            })
            .join_subscribe_with_state(hlist!(False, True), hlist!(False, True), |hlist_pat!(stuff), hlist_pat!(int_inp)| {
                int_inp - 1
            })
            .join_subscribe_with_state(hlist!(False, True, False), hlist!(False, True), |hlist_pat!(stuff), hlist_pat!(second_stage_inp)| {
                second_stage_inp - 1
            })
            .join_subscribe_with_state(hlist!(False, True, False, False), hlist!(True, False), |hlist_pat!(people), hlist_pat!(second_stage_inp)| {
                second_stage_inp + 1
            })
            .select_subscribe_with_state(hlist!(True, True, False, False, False), hlist!(True, True), |hlist_pat!(people, stuff), union_type| {
                union_type.fold(hlist!(|left_type| format!("{left_type}"), |right_type| format!("{right_type}")))
            });

        let data = graph.run().await;
        println!("{data:#?}");
    }

    /// In this test we don't actually want to do anything with the locks, we just want to test a
    /// traditional deadlock pattern
    #[tokio::test]
    async fn test_diamond_lock() {
        let stuff = new_shared(Vec::<String>::new());
        let people = new_shared(Vec::<Arc<Person>>::new());

        let graph = Graph {
            state: HNil,
            nodes: HNil,
        };

        let int_stream = stream::iter((0..100).into_iter());

        let graph = graph
            .add_state(stuff)
            .add_state(people.clone())
            .add_source_node(int_stream)
            .join_subscribe_with_state(hlist!(True), hlist!(True, False), |hlist_pat!(people), hlist_pat!(int_inp)| {
                int_inp + 1
            })
            .join_subscribe_with_state(hlist!(False, True), hlist!(False, True), |hlist_pat!(stuff), hlist_pat!(int_inp)| {
                int_inp - 1
            })
            .join_subscribe_with_state(hlist!(False, True, False), hlist!(False, True), |hlist_pat!(stuff), hlist_pat!(second_stage_inp)| {
                second_stage_inp - 1
            })
            .join_subscribe_with_state(hlist!(False, True, False, False), hlist!(True, False), |hlist_pat!(people), hlist_pat!(second_stage_inp)| {
                second_stage_inp + 1
            })
            .join_subscribe_with_state(hlist!(True, True, False, False, False), hlist!(True, True), |hlist_pat!(people, stuff), hlist_pat!(third_stage_inp_left, third_stage_inp_right)| {
                third_stage_inp_right + third_stage_inp_left
            });

        let data = graph.run().await;
        println!("{data:#?}");
    }
    #[tokio::test]
    async fn test_graph() {
        let stuff = new_shared(Vec::<String>::new());
        let people = new_shared(Vec::<Arc<Person>>::new());
        let scores = new_shared(HashMap::<Person, f32>::new());

        let graph = Graph {
            state: HNil,
            nodes: HNil,
        };

        let int_stream = stream::iter((0..100).into_iter());
        let char_stream = stream::iter(ALPHABET.chars());

        let data = graph
            .add_state(stuff)
            .add_state(people.clone())
            .add_state(scores)
            .add_source_node(int_stream)
            .add_source_node(char_stream)
            .join_subscribe(hlist!(True, False), |strm| strm.filter_map(move |hlist_pat!(x)| async move {if x == 'A' { Some(x)} else {None}}))
            // The following won't compile, because we don't have a uniform input guarantee on the
            // stream
            // .subscribe(hlist!(True, True, False), |input_stream| input_stream.take_while(|hlist_pat!(non_uniform_stream_item, _)| {
            //     let compare = *non_uniform_stream_item;
            //     async move {compare  == 'A'}
            // })
            // )
            .join_subscribe(hlist!(False, True, True), |input_stream| input_stream.take_while(|hlist_pat!(_, y)| {
                let compare = *y;
                async move {compare <= 10}
            })
            )
            .join_subscribe_with_state(hlist!(False, False, True, True), hlist!(False, True, True), |hlist_pat!(people, stuff), hlist_pat!(char_inp, int_inp)| {
                let name_opt = match char_inp {
                    'A' => Some("Alice".to_owned()),
                    'B' => Some("Bob".to_owned()),
                    _ => None,
                };
                if let Some(name) = name_opt {
                    let person =  Arc::new(Person {name, age: 30 + int_inp});
                    people.push(person.clone());
                    stuff.push(char_inp.to_string());
                    Some(person)
                } else {
                    None
                }

            })
            .join_subscribe(hlist!(True, False, False, True, True), |combined_stream| {
                combined_stream.filter_map(|hlist_pat!(person_opt, char_inp, int_inp)| async move {
                    if let Some(person) = person_opt {
                        Some((person, char_inp, int_inp))
                    } else {
                        None
                    }
                })
            })
            .run()
            .await;

        for person in people.lock().unwrap().iter() {
            println!("{person:#?}")
        }
        println!("head count {}", data.head);
        assert_eq!(data.tail, hlist!(26, 11, 1, 26, 100))
    }
}
