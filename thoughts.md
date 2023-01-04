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
library takes some of the same approaches: encoding the dependency of steps in the computational graph, lock groups guaranteeing lock order (although ours are weaker.
But until such a language is available, maybe we can build ways to more safely combine state and asynchronous stream processing without deadlocking.
Now despite the initial fear I decided to take the leap and look at async code, and have implemented a very rough PoC of an asynchronous stream processing library
that tries to guarantee deadlock free computation while allowine access to shared mutable state.

Some features of this library:
* Disallow locking primitives in streams being composed
  * this is done via the `LockFree` auto trait, which prevents locking primitives from being used in function types for the DAG
* Composition of streams as a DAG
  * all composition uses a Boolean HList, to filter previous nodes in the DAG as inputs for the current node
* Shared mutable state with guaranteed lock order
  * state is added to the DAG, and can be filtered via a similar subset mechanism. The lock order is is global
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
This means that highly confusing trait bound abound within. I am currently missing all documentation, but will slowly endeavour
to improve it. I have not formally proven that this library doesn't deadlock (and as is I don't think it does), but I believe
the recursive types and traits should allow proofs to be developed. But I think the formal proofs would actually reveal what additional
trait bounds are required.

## basic graph construction

I have provided an example which makes use of sqlx and in memory representations to calculate user profiles with exponential decay as a toy example.
```sh
cargo run --release --example transaction_in_mem
```
The example makes use of lock guarantees via the DAG.

What I noticed while implementing it is that in practice many asynchronous libraries use mutexes to manage shared resources, which may be incorporated into the futures of those libraries.
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
In this case the programmer promises that the stream is not messing around with mutexes in the future:
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
This can probably move to the library.

Another issue that came up is when developing, I tried to do too much in one step, 
and it was greatly simplified by moving more pieces into separate steps in the DAG.

# Tests
I attempted to provide some simple tests which illustrate various features
## non-uniform join
## diamond lock
## await while holding lock



Current shortcomings
* we don't exclude putting multiple clones of a single lock, which can trigger deadlock
* agressively avoid lifetimes in primitives; most types require `'static`
* missing common deadlock capable structs from library ecosystem (parking lot and tokio mutexes, dashmap, RWLocks)
* frunk structs may not have efficient layouts
* Unecessary copy of stream when node has only one subscriber
* formal specification and proof of guarantees
* no support for RWLocks which would allow better parallelism
* feels quite heavyweight with lots of channels, and clones
* ...?





