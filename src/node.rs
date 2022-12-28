use std::{marker::PhantomData, sync::mpsc::SendError};

use frunk::HNil;
use futures::{
    future::{ready, Ready},
    stream::{empty, repeat, Fold, Map, Repeat, Scan, Zip},
    Stream, StreamExt,
};
use tokio::sync::mpsc::Receiver;

use crate::{
    hierarchical_state::{AsMutHList, Filter, Lock, MutRefHList},
    sender::SenderHList,
    subscriber::{Subscribable, SubscriptionOutput},
};

pub struct UniformFlowStream<Inner>(Inner);
pub struct ArbitraryFlowStream<Inner>(Inner);

impl<Inner> ArbitraryFlowStream<Inner> {
    // todo: better type restriction
    pub fn new(inner: Inner) -> Self {
        ArbitraryFlowStream(inner)
    }
}

pub trait StreamWrapper {
    type Inner: Stream;
    fn inner(self) -> Self::Inner;
}

pub struct SourceStream<Inner>(Inner);
impl<Inner: Stream> SourceStream<Inner> {
    pub fn new(inner: Inner) -> Self {
        Self(inner)
    }
}

impl<Inner: Stream> StreamWrapper for SourceStream<Inner> {
    type Inner = Inner;
    fn inner(self) -> Self::Inner {
        self.0
    }
}
impl<Inner: Stream> StreamWrapper for ArbitraryFlowStream<Inner> {
    type Inner = Inner;
    fn inner(self) -> Self::Inner {
        self.0
    }
}

impl<Inner: Stream> StreamWrapper for UniformFlowStream<Inner> {
    type Inner = Inner;
    fn inner(self) -> Self::Inner {
        self.0
    }
}

pub struct Node<OutputType, Inner, OutputStream> {
    inner: Inner,
    output_stream: OutputStream,
    _output: PhantomData<OutputType>,
}

impl<OutputType, Inner> Node<OutputType, Inner, HNil> {
    pub fn new(inner: Inner) -> Self {
        Node {
            inner,
            output_stream: HNil,
            _output: PhantomData,
        }
    }
}

impl<OutputType, Inner, InnerStream, TailOutput> Node<OutputType, Inner, TailOutput>
where
    OutputType: Clone + 'static,
    TailOutput: Subscribable<OutputType> + SenderHList<OutputType>,
    Inner: StreamWrapper<Inner = InnerStream>,
    InnerStream: Stream<Item = OutputType>,
{
    pub fn subscribe(
        self,
    ) -> Subscription<Node<OutputType, Inner, TailOutput::NewSenders>, Receiver<OutputType>> {
        let Node {
            inner,
            output_stream,
            ..
        } = self;
        let SubscriptionOutput {
            new_senders,
            new_subscription,
        } = output_stream.subscribe();
        Subscription {
            node: Node {
                inner,
                output_stream: new_senders,
                _output: PhantomData::<OutputType>,
            },
            receiver: new_subscription,
        }
    }

    pub async fn run(self) -> usize {
        self.inner
            .inner()
            .then(|output| self.output_stream.send(output))
            .count()
            .await
    }
}

/// The stream transformation will be a function type taking a stream to a stream
/// the OutputStream will change as receivers are added.
///
pub struct AsyncInnerNode<InputType, OutputType, StreamTransformation, InputStream, InternalStream>
{
    _input: PhantomData<InputStream>,
    _transformation: PhantomData<StreamTransformation>,
    _input_type: PhantomData<InputType>,
    _output_type: PhantomData<OutputType>,
    internal_stream: InternalStream,
}

impl<
        InputType,
        InputStream: Stream<Item = InputType>,
        OutputType,
        InternalStream: Stream<Item = OutputType>,
        F: FnOnce(InputStream) -> InternalStream,
    > AsyncInnerNode<InputType, OutputType, F, InputStream, InternalStream>
{
    pub fn new(input_stream: InputStream, f: F) -> Self {
        AsyncInnerNode {
            _input: PhantomData,
            _transformation: PhantomData,
            _input_type: PhantomData,
            _output_type: PhantomData,
            internal_stream: f(input_stream),
        }
    }
}

pub trait GetInternalStream {
    type InternalStream;
    fn get_internal_stream(self) -> Self::InternalStream;
}

impl<InputType, OutputType, StreamTransformation, InputStream, InternalStream> GetInternalStream
    for AsyncInnerNode<InputType, OutputType, StreamTransformation, InputStream, InternalStream>
{
    type InternalStream = ArbitraryFlowStream<InternalStream>;
    fn get_internal_stream(self) -> Self::InternalStream {
        ArbitraryFlowStream(self.internal_stream)
    }
}

pub struct LockInnerNode<
    InputType,
    OutputType,
    ItemTransformation,
    LockType,
    InputStream,
    LockTypeAsMutRefHList,
    InternalStream,
> {
    _input: PhantomData<InputStream>,
    _transformation: PhantomData<ItemTransformation>,
    _input_type: PhantomData<InputType>,
    _output_type: PhantomData<OutputType>,
    _lock: PhantomData<LockType>,
    _lock_as_mut_ref_hlist: PhantomData<LockTypeAsMutRefHList>,
    internal_stream: InternalStream,
}

impl<
        InputType: 'static,
        OutputType: 'static,
        ItemTransformation: Clone,
        LockType: Clone + 'static,
        InputStream,
        LockTypeAsMutRefHList,
    >
    LockInnerNode<
        InputType,
        OutputType,
        ItemTransformation,
        LockType,
        InputStream,
        LockTypeAsMutRefHList,
        Scan<
            InputStream,
            (LockType, ItemTransformation),
            Ready<Option<OutputType>>,
            fn(&'_ mut (LockType, ItemTransformation), InputType) -> Ready<Option<OutputType>>,
        >,
    >
where
    LockTypeAsMutRefHList: MutRefHList,
    ItemTransformation:
        for<'a> Fn(LockTypeAsMutRefHList::MutRefHList<'a>, InputType) -> OutputType + Clone,
    InputStream: Stream<Item = InputType>,
    Self: 'static,
{
    pub fn new<FilterType: Filter>(
        input_stream: InputStream,
        lock: LockType,
        transformation: ItemTransformation,
    ) -> Self
    where
        LockType: Lock<FilterType, InnerType = LockTypeAsMutRefHList>,
    {
        LockInnerNode {
            _input: PhantomData,
            _transformation: PhantomData,
            _input_type: PhantomData::<InputType>,
            _output_type: PhantomData::<OutputType>,
            _lock: PhantomData,
            internal_stream: input_stream.scan(
                (lock.clone(), transformation),
                |(lock, f), item| {
                    let o = {
                        let mut guard = lock.lock();
                        guard.apply_fn(item, f.clone())
                    };
                    ready(Some(o))
                },
            ),
            _lock_as_mut_ref_hlist: PhantomData::<LockTypeAsMutRefHList>,
        }
    }
}

impl<
        InputType,
        OutputType,
        ItemTransformation,
        LockType,
        InputStream,
        LockTypeAsMutRefHList,
        InternalStream,
    > GetInternalStream
    for LockInnerNode<
        InputType,
        OutputType,
        ItemTransformation,
        LockType,
        InputStream,
        LockTypeAsMutRefHList,
        InternalStream,
    >
{
    type InternalStream = UniformFlowStream<InternalStream>;
    fn get_internal_stream(self) -> UniformFlowStream<InternalStream> {
        UniformFlowStream(self.internal_stream)
    }
}

impl<Inner> UniformFlowStream<Inner> {
    pub fn non_uniform(self) -> ArbitraryFlowStream<Inner> {
        ArbitraryFlowStream(self.0)
    }
}

pub struct Subscription<Node, Receiver> {
    pub node: Node,
    pub receiver: Receiver,
}

#[cfg(test)]
mod test {
    use std::{collections::HashMap, marker::PhantomData};

    use frunk::HNil;
    use frunk::{hlist, hlist_pat, HList};
    use futures::future::ready;
    use futures::stream::{self, empty, iter, repeat};
    use futures::{join, StreamExt};
    use tokio::sync::mpsc::Receiver;
    use tokio_stream::wrappers::ReceiverStream;

    use crate::node::GetInternalStream;
    use crate::util::{new_shared, SharedMutex};

    use super::{AsyncInnerNode, LockInnerNode};
    use super::{Node, Subscription};
    use crate::hierarchical_state::{AsMutHList, False, Lock, True};
    const ALPHABET_STR: &str = "abcdefghijklmnopqrstuvwxyz";

    #[tokio::test]
    async fn test_node_run() {
        let alphabet = iter(ALPHABET_STR.chars());
        let state_1 = new_shared(String::with_capacity(100));
        let state_2: SharedMutex<Vec<char>> = new_shared(Vec::with_capacity(200));
        let state_3: SharedMutex<HashMap<char, i32>> = new_shared(HashMap::new());

        let locks = hlist![state_1, state_2, state_3.clone(), state_3];

        let process = |hlist_pat![string, vector, hash_map]: HList!(&mut String, &mut Vec<char>, &mut HashMap<char, i32>),
                       input| {
            string.push(input);
            vector.push(input);
            if let Some(count) = hash_map.get_mut(&input) {
                *count += 1;
            } else {
                hash_map.insert(input, 1);
            }
            input
        };
        type Fltr = HList!(True, True, False, True);
        let node = LockInnerNode::new::<Fltr>(alphabet, locks.clone(), process);

        let data = node.internal_stream.collect::<Vec<_>>().await;
        println!("{locks:?}");
        assert_eq!(data, ALPHABET_STR.chars().collect::<Vec<_>>())
    }

    #[tokio::test]
    async fn test_async_node() {
        let even_alphabet = AsyncInnerNode::new(stream::iter(ALPHABET_STR.chars()), |stream| {
            stream
                .enumerate()
                .filter(|(number, _)| ready(*number % 2 == 0))
                .map(|(_, chr)| chr)
        })
        .internal_stream
        .collect::<Vec<_>>()
        .await;
        assert_eq!(
            even_alphabet,
            ALPHABET_STR.chars().step_by(2).collect::<Vec<_>>(),
        )
    }

    #[tokio::test]
    async fn test_node_subscription() {
        let alphabet = iter(ALPHABET_STR.chars());
        let state_1 = new_shared(String::with_capacity(100));
        let state_2: SharedMutex<Vec<char>> = new_shared(Vec::with_capacity(200));
        let state_3: SharedMutex<HashMap<char, i32>> = new_shared(HashMap::new());

        let locks = hlist![state_1, state_2, state_3.clone(), state_3];

        let process = |hlist_pat![string, vector, hash_map]: HList!(&mut String, &mut Vec<char>, &mut HashMap<char, i32>),
                       input| {
            string.push(input);
            vector.push(input);
            if let Some(count) = hash_map.get_mut(&input) {
                *count += 1;
            } else {
                hash_map.insert(input, 1);
            }
            input
        };
        type Fltr = HList!(True, True, False, True);
        let node = Node::new(
            LockInnerNode::new::<Fltr>(alphabet, locks.clone(), process).get_internal_stream(),
        );

        let Subscription {
            node,
            receiver: receiver1,
        }: Subscription<_, Receiver<char>> = node.subscribe();
        let Subscription {
            node,
            receiver: receiver2,
        }: Subscription<_, Receiver<char>> = node.subscribe();
        let first_run = ReceiverStream::new(receiver1).count();
        let second_run = ReceiverStream::new(receiver2).count();
        let (count_1, count_2, count_3) = join!(node.run(), first_run, second_run);
        println!("{count_1}, {count_2}, {count_3}");
        assert_eq!(count_1, count_2);
        assert_eq!(count_1, count_3);
    }
}
