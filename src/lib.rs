#![feature(auto_traits)]
#![feature(negative_impls)]
#![feature(trait_alias)]
#![feature(associated_type_defaults)]
mod async_coprod;
mod async_hlist;
mod hierarchical_state;
mod node;
mod receiver;
mod sender;
mod subscriber;
mod util;

#[cfg(test)]
mod tests {
    use std::collections::hash_map::DefaultHasher;
    use std::collections::HashSet;
    use std::hash::Hash;
    use std::hash::Hasher;
    use std::sync::Arc;

    use frunk::{hlist, hlist_pat};
    use futures::stream;
    use tokio::sync::mpsc::channel;
    use tokio::sync::mpsc::Sender;

    use crate::async_hlist::JoinFuture;
    use crate::receiver::JoinReceiverHList;
    use crate::receiver::SelectReceiverHList;
    use crate::sender::SenderHList;
    use crate::util::new_shared;

    use futures::{FutureExt, StreamExt};
    #[derive(Debug, Hash)]
    pub struct Message {
        text: String,
        id: i64,
    }
    impl Message {
        fn new(id: i64, text: String) -> Self {
            Message { text, id }
        }
    }
    const TAKE: usize = 30;

    async fn transmit<T: Send + std::fmt::Debug + Sync + 'static>(
        sender: anyhow::Result<Sender<T>>,
        t: T,
    ) -> anyhow::Result<Sender<T>> {
        let sender = sender?;
        sender.send(t).await?;
        Ok(sender)
    }

    #[tokio::test]
    async fn it_works() {
        let node_1_source = stream::repeat(1i64)
            .scan(0i64, |x, y| {
                *x += y;
                let z = *x;
                async move { Some(z) }
            })
            .map(|id| {
                println!("At id: {id}");
                Arc::new(Message {
                    text: format!("Got message {id}"),
                    id,
                })
            })
            .take(TAKE);

        let (tx_11, rx_11) = channel(2);
        let (tx_12, rx_12) = channel(2);
        let (tx_13, rx_13) = channel(2);

        let t1 = hlist![tx_11, tx_12, tx_13];

        let source_1 = node_1_source
            .fold(t1, |t1, message| {
                let message = message.clone();
                async move {
                    let res = t1.send(message).await;
                    let hlist_pat!(res_1, res_2, res_3) = res;
                    if vec![res_1, res_2, res_3].iter().any(|res| res.is_err()) {
                        println!("Failed!! ")
                    }
                    t1
                }
            })
            .map(|_| ());

        let node_2_source = stream::iter(1..=TAKE / 2).map(|i| i * i);

        let (tx_21, rx_21) = channel(2);
        let (tx_22, rx_22) = channel(2);

        let t2 = hlist![tx_21, tx_22];

        let source_2 = node_2_source
            .fold(t2, |t2, message| {
                let message = message.clone();
                async move {
                    t2.send(message).await;
                    t2
                }
            })
            .map(|_| ());

        let set_1 = HashSet::new();
        let set_2: HashSet<u64> = HashSet::new();

        let state_1 = new_shared(set_1);
        let state_1_copy = state_1.clone();

        let state_2 = new_shared(set_2);
        let state_2_copy = state_2.clone();

        let (tx_211, rx_211) = channel(2);
        let node_21 = hlist![rx_11, rx_21]
            .join_stream()
            .map(move |hlist_pat!(message, idx)| {
                let Message { text, id } = message.as_ref();
                let mut guard = state_1.lock().expect("Failed to lock in node_21");
                let found = guard.contains(&idx);
                if !found {
                    guard.insert(idx);
                };
                let new_text = format!("Merged {id}, {idx}, Old message:\"{text}\"");
                println!("Node_21: {new_text}");
                Arc::new(Message {
                    text: new_text,
                    id: *id,
                })
            })
            .fold(Ok(tx_211), transmit)
            .map(|_| ());

        let (tx_221, rx_221) = channel(2);
        let node_22 = hlist![rx_12, rx_22]
            .select_stream()
            .map(move |coprod| {
                let mut guard = state_2.lock().expect("Poisoned");
                use frunk::Coproduct::{Inl, Inr};

                let mut hasher = DefaultHasher::new();
                let id = match coprod {
                    Inl(message) => {
                        message.hash(&mut hasher);
                        hasher.finish()
                    } // message.text.hash(),
                    Inr(Inl(idx)) => idx as u64,
                    Inr(Inr(_cnil)) => unreachable!(),
                };
                let found = guard.contains(&id);
                if !found {
                    guard.insert(id);
                }
                println!("Node_22: {id}");
                id
            })
            .fold(Ok(tx_221), transmit)
            .map(|_| ());

        // let node_23 = todo!();

        let receivers = hlist![rx_211, rx_13, rx_221].select_stream().map(|r| {
            println!("Got {r:?}");
            r
        });

        let hlist_pat!(_, _, _, _, c) = hlist![
            source_1,
            source_2,
            node_21,
            node_22,
            receivers.collect::<Vec<_>>().map(|c| {
                println!("Collected");
                c
            })
        ]
        .join_fut()
        .await;
        println!("Vectors: {c:?}");
        assert_eq!(c.len(), 90);

        // todo
        // Make join nodes not work if intersection is non-empty
    }
}
