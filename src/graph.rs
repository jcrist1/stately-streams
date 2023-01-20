use std::pin::Pin;

use frunk::{prelude::HList, HCons, HNil};
use futures::{
    future::Map,
    Future, Stream, stream::{Scan, Then},
};
use tokio::task::{JoinError, JoinHandle};
use tokio_stream::wrappers::ReceiverStream;

use crate::{
    async_coprod::SelectSubscribable,
    async_hlist::{JoinFuture, JoinSubscribable},
    node::{
        ArbitraryFlowStream, AsyncInnerNode, Node, SourceStream, StreamWrapper, UniformFlowStream, Subscription, GetInternalStream, LockInnerNode, MapAsyncNode,
    },
    sender::SenderHList,
    subscriber::Subscribable, hierarchical_state::{True, False, MutRefHList, Lock, Filter}, util::{SafeType, LockFree},
};

pub struct Graph<State, Nodes> {
    state: State,
    nodes: Nodes,
}

impl Graph<HNil, HNil> {
    pub fn empty() -> Self {
        Self {
            state: HNil,
            nodes: HNil,
        }
    }
}

impl<TailState: HList, Nodes> Graph<TailState, Nodes> {
    pub fn add_state<HeadState>(self, head: HeadState) -> Graph<HCons<HeadState, TailState>, Nodes> {
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
    pub fn add_source_node<Item: SafeType, HeadNode: Stream<Item = Item>>(
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

pub trait UniformFlowStreamHList {}
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

pub trait SubscribableHList<Filter> {
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

impl<State: HList + Send + Sync, Nodes: HList> Graph<State, Nodes> {
    pub fn select_subscribe_with_state<
        NodeFilter: Filter,
        StateFilter: Filter,
        FilteredNodeOutputs: SafeType,
        SelectSubscriptionStream: Stream<Item = FilteredNodeOutputs> + 'static,
        FilteredNodesJoinSubscription: SelectSubscribable<SubscriptionStream = SelectSubscriptionStream>,
        FilteredNodes,
        Output: SafeType,
        FilteredState: MutRefHList + SafeType,
        F,
    >(
        self,
        // we have this to make type inference easier. Don't need to specify all type parameters
        _state_flter: StateFilter,
        _fltr: NodeFilter,
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
                        Map<JoinHandle<Output>, fn(Result<Output, JoinError>) -> Option<Output>>,
                        fn(&'_ mut (State, F), FilteredNodeOutputs) -> Map<JoinHandle<Output>, fn(Result<Output, JoinError>) -> Option<Output>>,
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
        State: Lock<StateFilter, InnerType = FilteredState> + Send + Sync,
        FilteredState: SafeType,
        FilteredNodeOutputs: SafeType,
        F: for<'a> Fn(FilteredState::MutRefHList<'a>, FilteredNodeOutputs) -> Output+ Clone + LockFree + Send + 'static,
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
    pub fn select_map_async<
        Filter,
        FilteredNodeOutputs: SafeType,
        SelectSubscriptionStream: Stream<Item = FilteredNodeOutputs>,
        FilteredNodesJoinSubscription: SelectSubscribable<SubscriptionStream = SelectSubscriptionStream>,
        FilteredNodes,
        Output: SafeType,
        FutureOutput: Future<Output = Output>,
        F: FnMut(FilteredNodeOutputs) -> FutureOutput
    >(
        self,
        _fltr: Filter,
        f: F
    ) -> Graph<
        State,
        HCons<
            Node<
                Output,
                ArbitraryFlowStream<Then<SelectSubscriptionStream, FutureOutput, F>>,
                HNil,
            >,
            <Nodes as SubscribableHList<Filter>>::NewSubscribedNodes,
        >
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
        let new_node = Node::new(
            // a select node, can't be joined safely with other nodes, as a subset of the original
            // nodes could be part of it, leading to non-uniform consumption
            MapAsyncNode::new(subscription.select_subscribe(), f).get_internal_stream().non_uniform()
        );
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

    pub fn select_subscribe<
        Filter,
        FilteredNodeOutputs: SafeType,
        SelectSubscriptionStream: Stream<Item = FilteredNodeOutputs>,
        FilteredNodesJoinSubscription: SelectSubscribable<SubscriptionStream = SelectSubscriptionStream>,
        FilteredNodes,
        Output: SafeType,
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
    pub fn join_subscribe_with_state<
        NodeFilter: Filter,
        StateFilter: Filter,
        FilteredNodeOutputs: SafeType,
        JoinSubscriptionStream: Stream<Item = FilteredNodeOutputs> + 'static,
        FilteredNodesJoinSubscription: JoinSubscribable<SubscriptionStream = JoinSubscriptionStream>,
        FilteredNodes: UniformFlowStreamHList,
        Output: SafeType,
        FilteredState: MutRefHList + SafeType,
        F,
    >(
        self,
        // we have this to make type inference easier. Don't need to specify all type parameters
        _state_flter: StateFilter,
        _fltr: NodeFilter,
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
                        Map<JoinHandle<Output>, fn(Result<Output, JoinError>) -> Option<Output>>,
                        fn(&'_ mut (State, F), FilteredNodeOutputs) -> Map<JoinHandle<Output>, fn(Result<Output, JoinError>) -> Option<Output>>,
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
        F: for<'a> FnMut(FilteredState::MutRefHList<'a>, FilteredNodeOutputs) -> Output+ Clone + LockFree + Send + 'static,
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

    pub fn join_map_async<
        Filter,
        FilteredNodeOutputs: SafeType,
        JoinSubscriptionStream: Stream<Item = FilteredNodeOutputs>,
        FilteredNodesJoinSubscription: JoinSubscribable<SubscriptionStream = JoinSubscriptionStream>,
        FilteredNodes: UniformFlowStreamHList,
        Output: SafeType,
        Fut: Future<Output = Output> + LockFree, 
        F: FnMut(FilteredNodeOutputs) -> Fut,
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
                UniformFlowStream
                <Then<JoinSubscriptionStream, Fut, F>>,
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
        let new_node = Node::new(MapAsyncNode::new(
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

    pub fn join_subscribe<
        Filter,
        FilteredNodeOutputs: SafeType,
        JoinSubscriptionStream: Stream<Item = FilteredNodeOutputs>,
        FilteredNodesJoinSubscription: JoinSubscribable<SubscriptionStream = JoinSubscriptionStream>,
        FilteredNodes: UniformFlowStreamHList,
        Output: SafeType,
        StreamOutput: Stream<Item = Output>,
        F: FnOnce(JoinSubscriptionStream) -> StreamOutput + LockFree,
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


trait NodeSubscriptionHList {}

#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use std::sync::Arc;

    use frunk::{hlist,  HNil, hlist_pat};
    use futures::future::ready;
    use futures::stream;
    use futures::stream::StreamExt;

    use crate::hierarchical_state::False;
    use crate::{util::new_shared, hierarchical_state::True};

    use super::Graph;

    #[derive(Debug, Clone, PartialEq, Hash)]
    struct Person {
        name: String,
        age: usize,
    }
    const ALPHABET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";

    #[tokio::test]
    async fn test_no_join_on_non_uniform() {

        let graph = Graph {
            state: HNil,
            nodes: HNil,
        };


        let int_stream = stream::iter((0..100).into_iter());

        let graph = graph
            .add_source_node(int_stream)
            .join_subscribe(hlist![True], |int_stream| int_stream.filter_map(|hlist_pat!(i)| {
                if i % 2 == 0 {
                    ready(Some(i))
                } else {
                    ready(None)
                }

            }))
            // the following will fail with: 
            //
            // rustc: the trait bound `HCons<ArbitraryFlowStream<futures::stream::FilterMap<futures::stream::Map<futures::stream::Zip<ReceiverStream<{integer}>, futures::stream::Repeat<HNil>>, fn(({integer}, HNil)) -> HCons<{integer}, HNil>>, futures::future::Ready<Option<{integer}>>, [closure@src/graph.rs:554:78: 554:93]>>, HCons<SourceStream<futures::stream::Iter<std::ops::Range<{integer}>>>, HNil>>: UniformFlowStreamHList` is not satisfied
            // the following other types implement trait `UniformFlowStreamHList`:
            //   HCons<SourceStream<Inner>, TailNodes>
            //   HCons<UniformFlowStream<Inner>, TailNodes>
            //
            // .join_subscribe(hlist!(True, True), |int_int_stream| int_int_stream.map(|hlist_pat!(i, j)| {
            //     i + j
            // }))
            ;

        let data = graph.run().await;
        println!("{data:#?}");
    }
    #[tokio::test]
    async fn test_mutex_in_stream_doesnt_compile() {
        let stuff = new_shared(Vec::<String>::new());
        let people = new_shared(Vec::<Arc<Person>>::new());

        let graph = Graph {
            state: HNil,
            nodes: HNil,
        };

        let _state_stream = stream::repeat(people);

        let int_stream = stream::iter((0..100).into_iter());

        let graph = graph
            .add_state(stuff)
            .add_source_node(int_stream)
            // the below fails with 
            // rustc: the trait bound `std::sync::Mutex<Vec<Arc<Person>>>: LockFree` is not satisfied in `[closure@src/graph.rs:555:68: 555:113]`
            // within `[closure@src/graph.rs:555:68: 555:113]`, the trait `LockFree` is not implemented for `std::sync::Mutex<Vec<Arc<Person>>>`
            // .join_subscribe_with_state(hlist!(True), hlist!(True), move |hlist_pat!(stuff), hlist_pat!(int_inp)| {
            //     let people = Arc::clone(&people);
            //     int_inp + 1
            // })
            //
            // The below fails with 
            // rustc: the trait bound `std::sync::Mutex<Vec<Arc<Person>>>: LockFree` is not satisfied in `Arc<std::sync::Mutex<Vec<Arc<Person>>>>`
            // required for `Arc<std::sync::Mutex<Vec<Arc<Person>>>>` to implement `util::SafeType`
            // .add_source_node(_state_stream)
            ;

        let data = graph.run().await;
        println!("{data:#?}");
    }
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
            .join_subscribe_with_state(hlist!(True, False), hlist!(True), |hlist_pat!(_people), hlist_pat!(int_inp)| {
                int_inp + 1
            })
            .join_subscribe_with_state(hlist!(False, True), hlist!(False, True), |hlist_pat!(_stuff), hlist_pat!(int_inp)| {
                int_inp - 1
            })
            .join_subscribe_with_state( hlist!(False, True), hlist!(False, True, False),|hlist_pat!(_stuff), hlist_pat!(second_stage_inp)| {
                second_stage_inp - 1
            })
            .join_subscribe_with_state(hlist!(True, False), hlist!(False, True, False, False), |hlist_pat!(_people), hlist_pat!(second_stage_inp)| {
                second_stage_inp + 1
            })
            .select_subscribe_with_state(hlist!(True, True), hlist!(True, True, False, False, False), |hlist_pat!(_people, _stuff), union_type| {
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
            .join_subscribe_with_state(hlist!(True, False), hlist!(True), |hlist_pat!(_people), hlist_pat!(int_inp)| {
                int_inp + 1
            })
            .join_subscribe_with_state(hlist!(False, True), hlist!(False, True), |hlist_pat!(_stuff), hlist_pat!(int_inp)| {
                int_inp - 1
            })
            .join_subscribe_with_state(hlist!(False, True), hlist!(False, True, False), |hlist_pat!(_stuff), hlist_pat!(second_stage_inp)| {
                second_stage_inp - 1
            })
            .join_subscribe_with_state(hlist!(True, False), hlist!(False, True, False, False), |hlist_pat!(_people), hlist_pat!(second_stage_inp)| {
                second_stage_inp + 1
            })
            .join_subscribe_with_state(hlist!(True, True), hlist!(True, True, False, False, False), |hlist_pat!(_people, _stuff), hlist_pat!(third_stage_inp_left, third_stage_inp_right)| {
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
            .join_subscribe_with_state(hlist!(False, True, True), hlist!(False, False, True, True), |hlist_pat!(people, stuff), hlist_pat!(char_inp, int_inp)| {
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
