use std::sync::Arc;

use frunk::{hlist, hlist_pat};
use futures::StreamExt;
use stately_streams::{
    graph::Graph,
    hierarchical_state::{False, True},
    util::new_shared,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    const LIMIT: usize = 1000000000;
    let source_one = futures::stream::iter(0..LIMIT)
        .chunks(1000000)
        .map(Arc::new);
    let twos = new_shared(Vec::<usize>::new());
    let threes = new_shared(Vec::<usize>::new());
    let fives = new_shared(Vec::<usize>::new());
    let graph = Graph::empty()
        .add_source_node(source_one)
        .add_state(Arc::clone(&twos))
        .add_state(Arc::clone(&threes))
        .add_state(Arc::clone(&fives))
        .join_subscribe_with_state(
            hlist!(False, False, True),
            hlist!(True),
            |hlist_pat!(twos), hlist_pat!(input)| {
                for i in input.iter() {
                    if i % 2 == 0 {
                        twos.push(*i)
                    }
                }
            },
        )
        .join_subscribe_with_state(
            hlist!(False, True, False),
            hlist!(False, True),
            |hlist_pat!(threes), hlist_pat!(input)| {
                for i in input.iter() {
                    if i % 3 == 0 {
                        threes.push(*i)
                    }
                }
            },
        )
        .join_subscribe_with_state(
            hlist!(True, False, False),
            hlist!(False, False, True),
            |hlist_pat!(fives), hlist_pat!(input)| {
                for i in input.iter() {
                    if i % 5 == 0 {
                        fives.push(*i)
                    }
                }
            },
        );

    println!("Starting graph");
    graph.run().await;
    let twos_raw = &(0..LIMIT)
        .into_iter()
        .filter(|x| x % 2 == 0)
        .collect::<Vec<_>>();
    println!("graph done");
    println!("checking twos");
    assert_eq!(twos_raw[..], twos.lock().unwrap()[..]);
    let threes_raw = &(0..LIMIT)
        .into_iter()
        .filter(|x| x % 3 == 0)
        .collect::<Vec<_>>();
    println!("checking thees");
    assert_eq!(threes_raw[..], threes.lock().unwrap()[..]);
    let fives_raw = &(0..LIMIT)
        .into_iter()
        .filter(|x| x % 5 == 0)
        .collect::<Vec<_>>();
    println!("checking fives");
    assert_eq!(fives_raw[..], fives.lock().unwrap()[..]);
    Ok(())
}
