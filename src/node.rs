use std::marker::PhantomData;

use futures::{
    stream::{repeat, Map, Repeat, Zip},
    Stream, StreamExt,
};
use tokio::sync::mpsc::Receiver;

use crate::hierarchical_state::{AsMutHList, Filter, Lock};

/// The stream transformation will be a function type taking a stream to a stream
/// the OutputStream will change as receivers are added.
///
pub struct AsyncNode<InputType, OutputType, StreamTransformation, InputStream, OutputStream> {
    input: InputStream,
    transformation: StreamTransformation,
    _input_type: PhantomData<InputType>,
    _output_type: PhantomData<OutputType>,
    output_stream: OutputStream,
}

pub struct LockNode<
    Filter,
    InputType,
    OutputType,
    ItemTransformation,
    LockType,
    InputStream,
    OutputStream,
> {
    input: InputStream,
    transformation: ItemTransformation,
    _input_type: PhantomData<InputType>,
    _output_type: PhantomData<OutputType>,
    lock: LockType,
    output_stream: OutputStream,
    _filter: PhantomData<Filter>,
}

pub struct Subscription<Node, Receiver> {
    node: Node,
    receiver: Receiver,
}

pub trait Node: Sized {
    type Input;
    type Output;
    type RunStream;
    fn subscribe(self) -> Subscription<Self, Receiver<Self::Output>>;

    fn run(self) -> Self::RunStream;
}

impl<
        InputType: 'static,
        OutputType: 'static,
        ItemTransformation: Clone,
        FilterType: Filter,
        LockType: Clone + 'static,
        InputStream,
        OutputStream,
    > Node
    for LockNode<
        FilterType,
        InputType,
        OutputType,
        ItemTransformation,
        LockType,
        InputStream,
        OutputStream,
    >
where
    LockType: Lock<FilterType>,
    for<'a> <LockType as Lock<FilterType>>::LockType<'a>: AsMutHList<'a>,
    ItemTransformation: for<'d> Fn(
        InputType,
        &'d mut <<LockType as Lock<FilterType>>::LockType<'d> as AsMutHList<'d>>::AsMutType<'d>,
    ) -> (
        OutputType,
        &'d mut <<LockType as Lock<FilterType>>::LockType<'d> as AsMutHList<'d>>::AsMutType<'d>,
    ),
    InputStream: Stream<Item = InputType>,
{
    type Input = InputType;

    type Output = OutputType;

    type RunStream = Map<
        Zip<Zip<InputStream, Repeat<LockType>>, Repeat<ItemTransformation>>,
        fn(((InputType, LockType), ItemTransformation)) -> OutputType,
    >;

    fn subscribe(self) -> Subscription<Self, Receiver<Self::Output>> {
        todo!()
    }

    fn run(self) -> Self::RunStream {
        let lock = self.lock;
        self.input
            .zip(repeat(lock))
            .zip(repeat(self.transformation))
            .map(lock_lock_type)
    }
}

fn lock_lock_type<
    InputType: 'static,
    OutputType: 'static,
    FilterType: Filter,
    LockType: Lock<FilterType>,
    F: for<'d> Fn(
        InputType,
        &'d mut <LockType::LockType<'d> as AsMutHList<'d>>::AsMutType<'d>,
    ) -> (
        OutputType,
        &'d mut <LockType::LockType<'d> as AsMutHList<'d>>::AsMutType<'d>,
    ),
>(
    ((input, lock), f): ((InputType, LockType), F),
) -> OutputType {
    let mut guard = lock.lock();
    let mut mut_ref = guard.mut_ref();
    let mut_mut_ref = &mut mut_ref;

    let (o, mut_mut_ref) = f(input, mut_mut_ref);
    drop(mut_ref);
    drop(guard);
    o
}

#[cfg(test)]
mod test {
    use std::{collections::HashMap, marker::PhantomData};

    use frunk::HNil;
    use frunk::{hlist, hlist_pat, HList};
    use futures::stream::{empty, iter, repeat};

    use crate::util::{new_shared, SharedMutex};

    use super::LockNode;
    use super::Node;
    use crate::hierarchical_state::{AsMutHList, Lock, True};

    #[test]
    fn test_node_run() {
        let alphabet = iter("abcdefghijklmnopqrstuvwxyz".chars());
        let state_1 = new_shared(String::with_capacity(100));
        let state_2: SharedMutex<Vec<char>> = new_shared(Vec::with_capacity(200));
        let state_3: SharedMutex<HashMap<char, i32>> = new_shared(HashMap::new());

        let locks = hlist![state_1, state_2, state_3];

        let process = |input, hlist_pat![string, vector, hash_map]: HList!(&mut String, &mut Vec<char>, &mut HashMap<char, i32>)| {
            string.push(input);
            vector.push(input);
            if let Some(count) = hash_map.get_mut(&input) {
                *count += 1;
            } else {
                hash_map.insert(input, 1);
            }
            input
        };
        let node = LockNode {
            input: alphabet,
            transformation: process,
            _input_type: PhantomData::<char>,
            _output_type: PhantomData::<char>,
            lock: locks,
            output_stream: empty::<()>(),
            _filter: PhantomData::<HList!(True, True, True)>,
        };
        // let run_stream = node.run();
    }
}
