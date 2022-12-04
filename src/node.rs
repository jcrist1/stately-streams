use std::marker::PhantomData;

use futures::{
    future::{ready, Ready},
    stream::{repeat, Fold, Map, Repeat, Zip},
    Stream, StreamExt,
};
use tokio::sync::mpsc::Receiver;

use crate::hierarchical_state::{AsMutHList, Filter, Lock, MutRefHList};

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
    LockTypeAsMutRefHList,
    OutputStream,
> {
    input: InputStream,
    transformation: ItemTransformation,
    _input_type: PhantomData<InputType>,
    _output_type: PhantomData<OutputType>,
    lock: LockType,
    _lock_as_mut_ref_hlist: PhantomData<LockTypeAsMutRefHList>,
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
        LockTypeAsMutRefHList,
        OutputStream,
    > Node
    for LockNode<
        FilterType,
        InputType,
        OutputType,
        ItemTransformation,
        LockType,
        InputStream,
        LockTypeAsMutRefHList,
        OutputStream,
    >
where
    LockTypeAsMutRefHList: MutRefHList,
    LockType: Lock<FilterType>,
    ItemTransformation:
        for<'a> Fn(LockTypeAsMutRefHList::MutRefHList<'a>, InputType) -> OutputType + Clone,
    InputStream: Stream<Item = InputType>,
    Self: 'static,
{
    type Input = InputType;

    type Output = OutputType;

    type RunStream = Fold<
        InputStream,
        Ready<(LockType, ItemTransformation)>,
        (LockType, ItemTransformation),
        fn((LockType, ItemTransformation), InputType) -> Ready<(LockType, ItemTransformation)>,
    >;

    fn subscribe(self) -> Subscription<Self, Receiver<Self::Output>> {
        todo!()
    }

    fn run(self) -> Self::RunStream {
        let lock = self.lock;
        self.input
            .fold((lock, self.transformation), |(lock, f), item| {
                let o = {
                    let reffed: &'_ LockType = &lock;
                    let mut guard: <LockType as Lock<FilterType>>::LockType<'_> = reffed.lock();
                    todo!()
                    // let o = guard.apply_fn(item, f.clone());
                    // o
                };
                ready((lock, f))
            })
        //.map(lock_lock_type)
    }
}

struct LockTypeAlignedFn<LockType, MutRefHListType, F, Fltr, Input, Output> {
    _locktype: PhantomData<LockType>,
    _mut_ref_hlist_type: PhantomData<MutRefHListType>,
    _filter: PhantomData<Fltr>,
    _input: PhantomData<Input>,
    _output: PhantomData<Output>,
    function: F,
}

impl<LockType, MutRefHListType, F, Fltr, Input, Output>
    LockTypeAlignedFn<LockType, MutRefHListType, F, Fltr, Input, Output>
where
    Fltr: Filter,
    LockType: Lock<Fltr>,
    MutRefHListType: MutRefHList,
    F: for<'e> Fn(MutRefHListType::MutRefHList<'e>, Input) -> Output,
{
    fn call<'a, 'b>(&self, input: Input, &'a LockType) -> Output where 
}

fn lock_and_apply<
    'a,
    InputType: 'static,
    OutputType: 'static,
    FilterType: Filter,
    MutRefHListType: MutRefHList,
    GuardType: for<'d> AsMutHList<'a, AsMutType<'d> = MutRefHListType::MutRefHList<'d>>,
    LockType: Lock<FilterType, LockType<'a> = GuardType>,
    ItemTransformation: for<'d> Fn(
            //<<LockType as Lock<FilterType>>::LockType<'a> as AsMutHList<'a>>::AsMutType<'d>,
            MutRefHListType::MutRefHList<'d>,
            InputType,
        ) -> OutputType
        + Clone,
>(
    lock: &'a LockType,
    f: ItemTransformation,
    input: InputType,
) -> OutputType {
    let reffed: &'_ LockType = &lock;
    let mut guard: <LockType as Lock<FilterType>>::LockType<'_> = reffed.lock();
    let o = guard.apply_fn(input, f.clone());
    o
}

//fn lock_lock_type<
//    InputType: 'static,
//    OutputType: 'static,
//    FilterType: Filter,
//    LockType: Lock<FilterType>,
//    F: for<'d> Fn(
//        InputType,
//        &'d mut <LockType::LockType<'d> as AsMutHList<'d>>::AsMutType<'d>,
//    ) -> (
//        OutputType,
//        &'d mut <LockType::LockType<'d> as AsMutHList<'d>>::AsMutType<'d>,
//    ),
//>(
//    ((input, lock), f): ((InputType, LockType), F),
//) -> OutputType {
//    let mut guard = lock.lock();
//    let mut mut_ref = guard.mut_ref();
//    let mut_mut_ref = &mut mut_ref;
//    let (o, mut_mut_ref) = f(input, mut_mut_ref);
//    drop(mut_ref);
//    drop(guard);
//    o
//}

#[cfg(test)]
mod test {
    use std::{collections::HashMap, marker::PhantomData};

    use frunk::HNil;
    use frunk::{hlist, hlist_pat, HList};
    use futures::stream::{empty, iter, repeat};
    use futures::StreamExt;

    use crate::util::{new_shared, SharedMutex};

    use super::LockNode;
    use super::Node;
    use crate::hierarchical_state::{AsMutHList, Lock, True};

    #[tokio::test]
    async fn test_node_run() {
        let alphabet = iter("abcdefghijklmnopqrstuvwxyz".chars());
        let state_1 = new_shared(String::with_capacity(100));
        let state_2: SharedMutex<Vec<char>> = new_shared(Vec::with_capacity(200));
        let state_3: SharedMutex<HashMap<char, i32>> = new_shared(HashMap::new());

        let locks = hlist![state_1, state_2, state_3];

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
        let node = LockNode {
            input: alphabet,
            transformation: process,
            _input_type: PhantomData::<char>,
            _output_type: PhantomData::<char>,
            lock: locks,
            output_stream: empty::<()>(),
            _filter: PhantomData::<HList!(True, True, True)>,
        };
        type Fltr = HList!(True, True, True);

        // let lock = node.lock;
        // let (_, _) = node
        //     .input
        //     .fold((lock, node.transformation), |(lock, f), item| {
        //         let o = {
        //             let mut guard = Lock::<Fltr>::lock(&lock);
        //             let o = guard.apply_fn(item, f.clone());
        //             // let _ = guard;
        //             o
        //         };
        //         futures::future::ready((lock, f))
        //     })
        //     .await;
        let run_stream = Node::run(node).await;
    }
}
