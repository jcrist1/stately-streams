# Thoughts from 2023-01
When I first started messing around with Rust some three-odd years back, I leaped at the fearless concurrency, and immediately 
coded up some nonsense shuffling stuff between threads. It didn't take long, however, to cause some deadlocks with mutexes.
Don't get me wrong, I was absolutely enamored by Rust's promises, and it fully delivered on those, but I was immediately
intrigued if the "next" language might be able to offer even stronger safety guarantees than Rust.

Later, I came upon [this article](https://blog.polybdenum.com/2022/07/24/fixing-the-next-thousand-deadlocks-why-buffered-streams-are-broken-and-how-to-make-them-safer.html)
on [reddit](https://www.reddit.com/r/rust/comments/w7g8js/lets_fix_buffered_streams/) and started [wondering](https://www.reddit.com/r/rust/comments/w7g8js/comment/ihjs75p/?utm_name=iossmf&context=3) 
if there's more that can be done with Rust's type system to guarantee deadlock free computation. One of the first ideas I had around that was by considering 
directed acyclic (computational) graph (DAG), which could be represented as a [partial order](https://en.wikipedia.org/wiki/Partially_ordered_set#Partial_order),
and encoding state dependendency as a partially ordered set via the subset relation, and mapping this with a partial order homomorphism.

Pretty quickly I singled out [Mutexes](https://www.reddit.com/r/rust/comments/wy84oh/can_we_create_deadlock_free_computation_in_the/) as a good first target.
I received a lot of positive feedback, including some encouragement to look into async code, but was immediately daunted by the potential complexity.

Before explaining what the library does I would like to mention a recent development in the literature (with which I am woefully unfamiliar): 
[Higher-Order Leak and Deadlock Free Locks](https://julesjacobs.com/pdf/locks.pdf). This really seems like it could contribute to "The Next Great Language". This 
library takes some of similar approaches: encoding the dependency of steps in the computational graph, lock groups guaranteeing lock order (although ours are guaranteed by a global order).
But until such a language is available, maybe we can build ways to more safely combine state and asynchronous stream processing without deadlocking.
Now despite the initial fear I decided to take the leap and look at async code, and have implemented a very rough PoC of an asynchronous stream processing library
that tries to guarantee deadlock free computation while allowine access to shared mutable state.

Some features of this library:
* Disallow locking primitives in streams being composed
  * this is done via the `LockFree` auto trait, which prevents locking primitives from being used in function types for the DAG
* Composition of streams as a DAG
  * all composition uses a Boolean HList, to filter previous nodes in the DAG as inputs for the current node
* Shared mutable state with guaranteed lock order
  * state is added to the DAG, and can be filtered via a similar subset mechanism. The lock order is global
* backpressure via finite tokio mpsc channels
  * values are passed between nodes via bounded async channels with capacity 2
* distinction between arbitrary stream transformations and transformations which guarantee a uniform progress 
  * We introduce a trait which guarantees that the filtered nodes will flow at a uniform rate: `UniformFlowHList`
* Support select merging arbitrary nodes in the graph as input for new nodes
  * for looser graph structure where we don't care about order, and consistent flow we allow select merging using frunks CoProduct
* and join merginging  nodes with the uniform flow guarantee
  * to avoid deadlocks from flows with different flow rates and shared parent nodes join merging is only supported with nodes satisfying the above trait

Most of these features make extensive use of Rust's ownership model and type system, and I have a hard time envisioning in being possible in other languages, 
without ownership semantics.

Finally this is really only a proof of concept at this stage. My main motiivation for this was to develop extremely complicated 
recursive traits which this library makes prolific use of, as well as the [frunk library](https://docs.rs/frunk/latest/frunk/).
This means that highly confusing trait bounds abound within. The code feels a bit like a hot mess and I am currently missing 
all documentation, but will slowly endeavour to improve both. I have not formally proven that this library doesn't deadlock (and as is I don't think it does), but I believe
the recursive types and traits should allow proofs to be developed. But I think the formal proofs would actually reveal what additional
trait bounds are required.

## How it works
Hidden behind almost everything is [Frunk](https://docs.rs/frunk/latest/frunk/)'s [HList](https://docs.rs/frunk/latest/frunk/hlist/trait.HList.html).
It is mostly used for the convenience macros `hlist!`, `HList!`, and `hlist_pat!`. 
A graph consists of an `HList` of nodes, and an HList of states, while a node consists of an internal stream as well as an `HList` of subscriber channel senders.
A subset of nodes can be subscribed to to create the input for a subsequent stream, and the streams can either be select merged in which case the input for the node
is a generic enum (`CoProduct`), or join merged, in which case the input is an `HList`. The input can then be transformed in several ways, we can do a generic stream
transformation `FnOnce(InputStream: Stream) -> OutputStream: Stream`), an asynchronous function `FnMut(InputType) -> Fut: Future<Output = OutputType>`) or
a stately transformation `FnMut(&mut StateType, InputType) -> OutputType`.

Given a graph we can we can add either a new source node from an asynchronous stream, or subscribe to previously existing nodes.
Subscribing to existing nodes, requires filtering the nodes, which is done with an HList of  custom boolean structs `True` and `False` which implement a custom `Boolean` trait.
This could probably work with const generics, but I didn't expect it would play nicely with frunk and so didn't try. Subscriptions can either be join:
waiting for all subscribed nodes to produce an element, or select: providing an elemnt from the fastest stream.
In the case of the former the input type for the stream will be an HList (or [product](https://en.wikipedia.org/wiki/Product_(category_theory)), essentially a tuple)
of the output types of each subscribed nodes, while in the latter, it will be a [coproduct](https://en.wikipedia.org/wiki/Coproduct) 
(essentially an enum). Finally we attach multiple pieces of state to the graph (currently each piece only be `Arc<Mutex<_>>`).
Inside the graph, each piece of state is appended to an HList containing all of the state of the graph, and when we add a node,
we need to decide whether to use state and which pieces. If we wish to do this we need a second filter. An example 
```rust
graph.join_subscribe_with_state(
    hlist![True, False, True, True, False],
    hlist![False, True, True, False, False],
    |hlist_pat(state_1, state_2, state_3),
     hlist_pat!(node_2_output, node_3_output)| {
         // code that updates state and transforms inputs to output type
         ...
         output
     }
)
```

To get a taste of how it works, consider an example method:
```rust
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
        F: for<'a> FnMut(FilteredState::MutRefHList<'a>, FilteredNodeOutputs) -> Output+ Clone + LockFree + 'static,
    {
    ...
}
```
Most of the engineering is in the traits and type constraints. Let's look at those:
* `Filter` is for types which are HLists of `TypeBool` types
* `SafeType` is `LockFree + Clone + Send + Sync + Sized + 'static`, and is supposed to capture a the idea of a "simple" or "safe" type,
that it should be okay to pass around within the graph (or hold as state)
* `JoinSubscribable` means that the HList of nodes that are to be subscribed to, can be subscribed to
* `UniformFlowStreamHlist` is for node HLists that can be safely join subscribed
* `MutRefHList` means the type can be recursively dereferenced into an hlist of mutable references
* `GetInternalStream` is just for getting an internal stream to a node and wrapping it in a struct that indicates whether it can be safely join subscribed
* `SubscribableHList<NodeFilter>` is for hlists of nodes that can be filtered with the given `Filter`, `NodeFilter` (essentially that the filter and the nodes have the same length).
* `Lock<StateFilter>` is similar to `SubscribableHList<_>` but for locking an hlist of `Arc<Mutex<_>>` types to get mutable references.

## basic graph construction

I have provided an example which makes use of sqlx and in memory representations to calculate user profiles with exponential decay as a toy example.
You will need a running postgres instance, which can be spun up with 
```sh
docker-compose up -d postgres
```
then run migrations
```sh
sqlx migrate run
```
and finally 
```sh
cargo run --release --example transaction_in_mem
```
The example makes use of transaction guarantees via the DAG: we can ensure that an object is in memory and that a later flow step cannot change the state by binding to the initial source stream.

What I noticed while implementing it is that in practice many asynchronous libraries use mutexes to manage shared resources, which may be incorporated into the futures those libraries generate.
My original example failed to use sqlx because of multiple mutexes in various types:
```rust
  .join_map_async(
      hlist![True, False],
      |hlist_pat![maybe_missing_user_event]| async {
          match maybe_missing_user_event {
              Some(missing_user_event) => {
                  let PersonalisationEvent { user_id, .. } = missing_user_event.as_ref();
                  let pool = Arc::clone(&pool);
                  sqlx::query_as!(
                      User,
                      r#"
                          select id, username as name, updated as last_updated from users
                          where id = $1
                          "#,
                      user_id
                  )
                  .fetch_one(&*pool)
                  .await
                  .map_err(|_| Error::PgError)
                  .map(|_| ())
              }
              None => Ok::<(), Error>(()),
          }
      },
  );
```
![rustc: the trait bound `std::sync::Mutex<event_listener::List>: LockFree` is not satisfied in `impl futures::Future<Output = Result<(), Error>>`
required because it appears within the type `*mut event_listener::Inner`
required because it appears within the type `&Arc<Pool<Postgres>>`
rustc: the trait bound `std::sync::Mutex<tokio::util::slab::Slots<runtime::io::scheduled_io::ScheduledIo>>: LockFree` is not satisfied in `impl futures::Future<Output = Result<(), Error>>`
required because it appears within the type `*const tokio::util::slab::Page<runtime::io::scheduled_io::ScheduledIo>`
required because it appears within the type `*const tokio::util::slab::Value<runtime::io::scheduled_io::ScheduledIo>`
required because it appears within the type `[crossbeam_queue::array_queue::Slot<pool::connection::Idle<Postgres>>]`
required because it appears within the type `&Arc<Pool<Postgres>>`
rustc: the trait bound `std::sync::Mutex<runtime::io::scheduled_io::Waiters>: LockFree` is not satisfied in `impl futures::Future<Output = Result<(), Error>>`
required because it appears within the type `*const tokio::util::slab::Value<runtime::io::scheduled_io::ScheduledIo>`
required because it appears within the type `[crossbeam_queue::array_queue::Slot<pool::connection::Idle<Postgres>>]`
required because it appears within the type `&Arc<Pool<Postgres>>`
rustc: the trait bound `(dyn for<'a, 'b> Fn(&'a mut [u8], &'b PgTypeInfo) + std::marker::Send + Sync + 'static): LockFree` is not satisfied in `impl futures::Future<Output = Result<(), Error>>`
within `impl futures::Future<Output = Result<(), Error>>`, the trait `LockFree` is not implemented for `(dyn for<'a, 'b> Fn(&'a mut [u8], &'b PgTypeInfo) + std::marker::Send + Sync + 'static)`
rustc: the trait bound `dyn futures::Future<Output = Result<Option<PgRow>, sqlx::Error>> + std::marker::Send: LockFree` is not satisfied in `impl futures::Future<Output = Result<(), Error>>`
within `impl futures::Future<Output = Result<(), Error>>`, the trait `LockFree` is not implemented for `dyn futures::Future<Output = Result<Option<PgRow>, sqlx::Error>> + std::marker::Send`
rustc: the trait bound `(dyn std::error::Error + std::marker::Send + Sync + 'static): LockFree` is not satisfied in `impl futures::Future<Output = Result<(), Error>>`
within `impl futures::Future<Output = Result<(), Error>>`, the trait `LockFree` is not implemented for `(dyn std::error::Error + std::marker::Send + Sync + 'static)`
rustc: the trait bound `(dyn rustls::tls12::cipher::Tls12AeadAlgorithm + 'static): LockFree` is not satisfied in `impl futures::Future<Output = Result<(), Error>>`
required because it appears within the type `&'static (dyn rustls::tls12::cipher::Tls12AeadAlgorithm + 'static)`
required because it appears within the type `&'static rustls::tls12::Tls12CipherSuite`
required because it appears within the type `[crossbeam_queue::array_queue::Slot<pool::connection::Idle<Postgres>>]`
required because it appears within the type `&Arc<Pool<Postgres>>`
rustc: the trait bound `(dyn rustls::cipher::MessageEncrypter + 'static): LockFree` is not satisfied in `impl futures::Future<Output = Result<(), Error>>`
required because it appears within the type `[crossbeam_queue::array_queue::Slot<pool::connection::Idle<Postgres>>]`
required because it appears within the type `&Arc<Pool<Postgres>>`
rustc: the trait bound `(dyn for<'a> Fn(&'a mut PgConnection, PoolConnectionMetadata) -> Pin<Box<dyn futures::Future<Output = Result<(), sqlx::Error>> + std::marker::Send>> + std::marker::Send + Sync + 'static): LockFree` is not satisfied in `impl futures::Future<Output = Result<(), Error>>`
required because it appears within the type `&Arc<Pool<Postgres>>`
rustc: the trait bound `(dyn rustls::cipher::MessageDecrypter + 'static): LockFree` is not satisfied in `impl futures::Future<Output = Result<(), Error>>`
required because it appears within the type `[crossbeam_queue::array_queue::Slot<pool::connection::Idle<Postgres>>]`
required because it appears within the type `&Arc<Pool<Postgres>>`
rustc: the trait bound `(dyn for<'a> Fn(&'a mut PgConnection, PoolConnectionMetadata) -> Pin<Box<dyn futures::Future<Output = Result<bool, sqlx::Error>> + std::marker::Send>> + std::marker::Send + Sync + 'static): LockFree` is not satisfied in `impl futures::Future<Output = Result<(), Error>>`
required because it appears within the type `&Arc<Pool<Postgres>>`
rustc: the trait bound `(dyn rustls::conn::State<rustls::client::client_conn::ClientConnectionData> + 'static): LockFree` is not satisfied in `impl futures::Future<Output = Result<(), Error>>`
required because it appears within the type `[crossbeam_queue::array_queue::Slot<pool::connection::Idle<Postgres>>]`
required because it appears within the type `&Arc<Pool<Postgres>>`
rustc: the trait bound `(dyn DatabaseError + 'static): LockFree` is not satisfied in `impl futures::Future<Output = Result<(), Error>>`
within `impl futures::Future<Output = Result<(), Error>>`, the trait `LockFree` is not implemented for `(dyn DatabaseError + 'static)`](images/sqlx-not-lockfree-type-error.png)

The solution for this was to implement a wrapper type for futures that allows unsafe declaration of lock free behaviour.
In this case the programmer promises that the stream is not messing around with locks in the future:
```rust
#[pin_project]
struct UnsafeLockFreeFut<Fut>(#[pin] Fut);

impl<F: Future> UnsafeLockFreeFut<F> {
    unsafe fn new(f: F) -> Self {
        Self(f)
    }
}

unsafe impl<Fut> LockFree for UnsafeLockFreeFut<Fut> {}

impl<Fut, Output> Future for UnsafeLockFreeFut<Fut>
where
    Fut: Future<Output = Output>,
{
    type Output = Output;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let this = self.project();
        this.0.poll(cx)
    }
}
```
This should probably move to the library.

Another issue that came up is while developing this example: I tried to do too much in one dag step, 
and it was greatly simplified by moving more pieces into separate nodes in the DAG.

# Tests
I attempted to provide some simple tests which illustrate various features. I hesitate to call these tests, as I wrote them to merely test that the test compiles,
however they include commented code that when uncommented causes the test to fail to compile.
## non-uniform join
A test to verify that a graph with two uneven paths can't be joined in `graph::test::test_no_join_on_non_uniform`.
The corresponding failure mode would be that we attempt to take a stream, send it down two channels, filter every other element of the second channel,
and then join the streams from the channels.
## diamond lock
We test that if we have two mutexes, and two paths, locking them in different order doesn't deadlock, either in uniform or non-uniform flows:
`graph::test::{test_diamond_lock_select, test_diamond_lock}`
## await while holding lock
All function types are required to be `LockFree`, which (should) prohibit the existence of locking primitives in them.
This was proven nicely by surprise when I was implementing the example code when I discovered that there are mutexes
in multiple places in sqlx (which in retrospect is not surprising). This is tested in `graph::test::test_mutex_in_stream_doesnt_compile`



# Current shortcomings
* code is a bit of a hot mess with little to no documentation
* tests of all graph construction functions
* we don't exclude putting multiple clones of a single lock, which can trigger deadlock
  * A potential solution is to make adding state outside of a mutex safe, and adding state in a mutex unsafe
* agressively avoid lifetimes in primitives; most types require `'static`
* missing common deadlock capable structs from library ecosystem (parking lot and tokio mutexes, dashmap, RWLocks)
* frunk structs may not have efficient layouts
* Unecessary copy of stream when node has only one subscriber
* no formal specification and proof of guarantees
* no support for RWLocks which would allow better parallelism/concurrency
* feels quite heavyweight with lots of channels, and clones
* there is no ergonomic way to subscribe to the output of the graph, or retrieve the state.
* no ergonomic support for fallible streams (e.g. no early termination, retries, ...?)
* probably much more ...?

# (Dis-)honorable notes:
As mentioned, this makes extensive use of inductive reasoning, via traits. The type parameters, constraints and signature of
```rust
    pub fn join_subscribe_with_state<...>(...

```
are 2.5 times longer than the body.
My friend said of this
> I hereby declare this the spookiest shit I've seen in rust

which I take a certain dubious pride in.



