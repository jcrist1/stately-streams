use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, HashMap},
    iter::once,
    sync::{Arc, Mutex},
};

use anyhow::anyhow;
use frunk::{hlist, hlist_pat};
use futures::{future::ready, stream, Future};
use pin_project::pin_project;
use rand::{distributions::Uniform, prelude::*, thread_rng};
use serde::Deserialize;
use sqlx::{postgres::PgPoolOptions, types::time::OffsetDateTime, PgPool};
use stately_streams::{graph::Graph, util};
use stately_streams::{hierarchical_state::*, util::LockFree};
use time::{Date, Duration};

#[derive(Clone, Deserialize)]
struct User {
    id: i64,
    name: String,
    last_updated: OffsetDateTime,
}

const USER_TOPIC_SCORE_DAY_DECAY: f64 = 0.999;
const SECS_PER_DAY: f64 = 86400.0;

#[derive(Debug, Clone, PartialEq)]
struct ScoredUser {
    user_id: i64,
    score: f64,
    last_action: OffsetDateTime,
}
impl ScoredUser {
    fn update_score<
        'a,
        I: Iterator<Item = &'a (UserId, OffsetDateTime, PersonalisationEventType)>,
    >(
        &'a mut self,
        events_iter: I,
    ) {
        for (_user_id, event_date, event_type) in events_iter {
            let diff = *event_date - self.last_action;
            let scale_offset =
                USER_TOPIC_SCORE_DAY_DECAY.powf(diff.as_seconds_f64() / SECS_PER_DAY);
            self.score *= scale_offset;
            self.score += event_type.to_score();
            self.last_action = *event_date;
        }
    }
}

impl Eq for ScoredUser {}

type UserId = i64;
type TopicEvents = BTreeMap<Topic, BTreeSet<(UserId, OffsetDateTime, PersonalisationEventType)>>;
type TopicView = BTreeMap<Topic, BTreeSet<ScoredUser>>;

impl PartialOrd for ScoredUser {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        let diff = self.last_action - other.last_action;
        let adjusted_score =
            other.score * USER_TOPIC_SCORE_DAY_DECAY.powf(diff.as_seconds_f64() / SECS_PER_DAY);
        match PartialOrd::partial_cmp(&self.score, &adjusted_score) {
            Some(std::cmp::Ordering::Equal) => Some(Ord::cmp(&self.user_id, &other.user_id)),
            Some(ordering) => Some(ordering),
            None => None,
        }
    }
}

impl Ord for ScoredUser {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let diff = self.last_action - other.last_action;
        let adjusted_score =
            other.score * USER_TOPIC_SCORE_DAY_DECAY.powf(diff.as_seconds_f64() / SECS_PER_DAY);
        match PartialOrd::partial_cmp(&self.score, &adjusted_score) {
            Some(std::cmp::Ordering::Equal) => Ord::cmp(&self.user_id, &other.user_id),
            Some(ordering) => ordering,
            None => Ord::cmp(&self.user_id, &other.user_id),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Topic(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PersonalisationEventType {
    Subscribe,
    Search,
    View,
}

const EVENT_TYPES: [PersonalisationEventType; 3] = [
    PersonalisationEventType::Subscribe,
    PersonalisationEventType::Search,
    PersonalisationEventType::View,
];

impl PersonalisationEventType {
    fn to_score(&self) -> f64 {
        match self {
            PersonalisationEventType::Subscribe => 0.9,
            PersonalisationEventType::Search => 0.1,
            PersonalisationEventType::View => 0.05,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PersonalisationEvent {
    user_id: i64,
    event_type: PersonalisationEventType,
    topic: Topic,
    event_time: OffsetDateTime,
}

#[derive(thiserror::Error, Debug, Clone)]
pub enum Error {
    #[error("Error from DB, {pg_error_string}")]
    DbError { pg_error_string: String },
    #[error("Missing {0}")]
    Missing(String),
}
impl From<sqlx::Error> for Error {
    fn from(value: sqlx::Error) -> Self {
        Error::DbError {
            pg_error_string: format!("{value}"),
        }
    }
}

async fn insert_user(user: &User, client: &PgPool) -> Result<User, Error> {
    Ok(sqlx::query_as!(
        User,
        r#"
            insert into users (
                username,
                password,
                created,
                updated
            ) values (
                $1,
                'password',
                $2,
                $2
            ) 
            returning id, username as name, updated as last_updated
        "#,
        user.name,
        user.last_updated
    )
    .fetch_one(client)
    .await?)
}

async fn delete_users(client: &PgPool) -> Result<(), Error> {
    sqlx::query!(
        r#"
            delete from users 
        "#,
    )
    .execute(client)
    .await?;
    Ok(())
}

async fn get_user(user_id: i64, client: &PgPool) -> Result<User, Error> {
    Ok(sqlx::query_as!(
        User,
        r#"
            select 
                id,
                username as name,
                updated as last_updated
            from users
            where id = $1
        "#,
        user_id
    )
    .fetch_one(client)
    .await?)
}

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
const NAMES: [&str; 12] = [
    "Alice", "Bob", "Charlie", "Dylan", "Eric", "Fiona", "Gilbert", "Hilda", "Isolde", "Julia",
    "Kappa", "Lauren",
];

struct NameSampler;

impl Distribution<&'static str> for NameSampler {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> &'static str {
        let idx = Uniform::<usize>::new(0, NAMES.len()).sample(rng);
        NAMES[idx]
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pool = Arc::new(
        PgPoolOptions::new()
            .connect("postgresql://stream_user:secret@localhost:5436/stream_example")
            .await?,
    );
    let topic_view: TopicView = BTreeMap::new();
    let mut users = BTreeMap::<UserId, User>::new();
    // happy path: events arrive in chronological order.
    let mut rng = thread_rng();
    let dist = Uniform::<UserId>::new(i64::MIN, i64::MAX);
    const USERS: usize = 1000;
    let mut user_ids = Vec::with_capacity(USERS);
    for user in (0..USERS).map(|row| {
        if row % 100 == 0 {
            println!("Inserted {row} users")
        }
        let user_id = dist.sample(&mut rng);
        let name = NameSampler.sample(&mut rng).to_owned();
        User {
            id: user_id,
            name,
            last_updated: OffsetDateTime::UNIX_EPOCH,
        }
    }) {
        let user = insert_user(&user, pool.as_ref()).await?;
        user_ids.push(user.id);
        if user.id % 2 == 0 {
            users.insert(user.id, user);
        }
    }

    const EVENTS: usize = 1_000_000;
    let exp = rand_distr::Exp::new(0.001)?; //  ::distributions:: Exponential::new(1000.0);
    let now = OffsetDateTime::now_utc();
    let dates = (0..EVENTS)
        .map(|event_no| {
            if event_no % 10_000 == 0 {
                println!("inserted {event_no} event dates");
            }
            let offset = rng.sample(exp);

            let duration = Duration::seconds_f64(offset * SECS_PER_DAY);
            now - duration
        })
        .collect::<BTreeSet<_>>();

    let topics = [
        "Cat",
        "Dog",
        "Horse",
        "Beet",
        "Cabbage",
        "Chocolate",
        "Wood",
        "Metal",
        "Technology",
        "Literature",
    ]
    .into_iter()
    .map(String::from)
    .map(|string| Topic(string))
    .collect::<Vec<_>>();

    println!("Creating personalisation events");
    let personalisation_events = dates
        .into_iter()
        .map(move |event_time| -> anyhow::Result<PersonalisationEvent> {
            let user_id = *user_ids
                .choose(&mut rng)
                .ok_or_else(|| anyhow!("tried to sample from empty users"))?;
            let event_type = *EVENT_TYPES
                .choose(&mut rng)
                .ok_or_else(|| anyhow!("wish the compiler new this was non-empty"))?;
            let topic = topics
                .choose(&mut rng)
                .ok_or_else(|| anyhow!("definitely should have a nonempty list of topics"))?
                .clone();
            Ok(PersonalisationEvent {
                event_time,
                event_type,
                topic,
                user_id,
            })
        })
        .filter_map(|event| event.ok())
        .map(Arc::new);
    println!("Done");

    let personalisation_event_store: TopicEvents = BTreeMap::new();
    let delete_pool = pool.clone();
    let mut counter = 0;
    let topic_view = util::new_shared(topic_view);
    let user_view: Arc<Mutex<HashMap<UserId, HashMap<Topic, (OffsetDateTime, f64)>>>> =
        util::new_shared(HashMap::new());

    let graph = Graph::empty()
        .add_state(util::new_shared(users))
        .add_state(util::new_shared(personalisation_event_store))
        .add_state(topic_view.clone())
        .add_state(user_view)
        .add_source_node(stream::iter(personalisation_events.into_iter()))
        // insert
        .join_subscribe_with_state(
            hlist![True],
            hlist![False, False, False, True],
            move |hlist_pat![users], hlist_pat![event_arc]| {
                if counter % 10_000 == 0 {
                    println!("Processing event {counter}");
                }
                counter += 1;
                let PersonalisationEvent {
                    user_id,
                    event_time,
                    ..
                } = event_arc.as_ref();
                match users.get_mut(&user_id) {
                    Some(user) => {
                        let event_processing = if user.last_updated >= *event_time {
                            EventOrder::OutOfOrder
                        } else {
                            EventOrder::InOrder {
                                previous_action: user.last_updated,
                            }
                        };
                        user.last_updated = user.last_updated.max(*event_time);
                        Some((user.clone(), event_processing))
                    }
                    None => None,
                }
            },
        )
        // get user from network
        .join_map_async(
            hlist![True, True],
            move |hlist_pat![found_user, user_event]| {
                let pool = pool.clone();
                let fut = async move {
                    match found_user {
                        None => get_user(user_event.user_id, pool.as_ref())
                            .await
                            .map(|user| {
                                let event_order_processing =
                                    if user.last_updated >= user_event.event_time {
                                        EventOrder::OutOfOrder
                                    } else {
                                        EventOrder::InOrder {
                                            previous_action: user.last_updated,
                                        }
                                    };
                                (Some(user), event_order_processing)
                            }),
                        Some((_, event_processing)) => {
                            Ok::<(Option<User>, EventOrder), Error>((None, event_processing))
                        }
                    }
                };
                unsafe { UnsafeLockFreeFut::new(fut) }
            },
        )
        // store user if missing, and store userscores
        .join_subscribe_with_state(
            hlist![True, False, True],
            hlist![False, False, True, False],
            |hlist_pat![personalisation_events],
             hlist_pat![user_inserted, user_event]|
             -> Result<Arc<PersonalisationEvent>, Error> {
                user_inserted?;
                let data = (
                    user_event.user_id,
                    user_event.event_time,
                    user_event.event_type,
                );
                match personalisation_events.get_mut(&user_event.topic) {
                    Some(topic_events) => {
                        topic_events.insert(data);
                    }
                    None => {
                        personalisation_events.insert(
                            user_event.topic.clone(),
                            once(data).collect::<BTreeSet<_>>(),
                        );
                    }
                }
                Ok(user_event)
            },
        )
        .join_subscribe_with_state(
            hlist![False, True, False, False],
            hlist![False, False, False, True],
            |
             hlist_pat![users],hlist_pat![user_to_insert] | -> Result<(), Error> {
                 if let (Some(user), _) =  user_to_insert? {
                         users.insert(user.id, user);
                 }
                 Ok(())
             }
        )
        .join_subscribe_with_state(
            hlist![False, True, True, False, True],
            hlist![True, True, True, False],
            |hlist_pat![users_view, topic_view, personalisation_events],
             hlist_pat![_user_inserted, user_for_insert, event]| -> Result<(), Error> {
                 insert(users_view, topic_view, personalisation_events, user_for_insert, event.as_ref())

             }
            )
        // .join_subscribe_with_state(
        //     hlist![True, False, False, True],
        //     hlist![True, True, False, True],
        //     |hlist_pat!(users_view, topic_view, users),
        //      hlist_pat!(user_to_insert_option_result, user_event)| {
        //         insert(
        //             users_view,
        //             users,
        //             personalisation_events,
        //             topic_view,
        //             user_to_insert_option_result,
        //             user_event.as_ref(),
        //         )
        //     },
        // )
        ;

    println!("Running graph");
    graph.run().await;
    println!("Done");

    println!("Clearing DB");
    delete_users(delete_pool.as_ref()).await?;
    println!("Done");
    let user_scores = &*topic_view.lock().map_err(|_| anyhow!("Poisoned lock"))?;
    for (topic, scores) in user_scores.iter() {
        println!("User scores for {topic:?}");
        println!("{:#?}", scores);
    }

    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub enum EventOrder {
    OutOfOrder,
    InOrder { previous_action: OffsetDateTime },
}
const MIN_DATE: OffsetDateTime = Date::MIN.midnight().assume_utc();
const MAX_DATE: OffsetDateTime = Date::MAX.midnight().assume_utc();

fn insert(
    user_view: &mut HashMap<UserId, HashMap<Topic, (OffsetDateTime, f64)>>,
    topic_view: &mut TopicView,
    personalisation_events: &mut TopicEvents,
    user_for_insert: Result<(Option<User>, EventOrder), Error>,
    PersonalisationEvent {
        user_id,
        event_type,
        topic,
        event_time,
    }: &PersonalisationEvent,
) -> Result<(), Error> {
    let (_, event_order) = user_for_insert?;
    let default_user = ScoredUser {
        user_id: *user_id,
        score: 0.0,
        last_action: MIN_DATE,
    };
    let topic_events = personalisation_events
        .get(&topic)
        .ok_or_else(|| Error::Missing(format!("Topic Events: {topic:?}")))?;
    let events_iter = match event_order {
        EventOrder::OutOfOrder => topic_events
            .range((*user_id, MAX_DATE, *event_type)..=(*user_id, *event_time, *event_type)),
        EventOrder::InOrder { previous_action } => topic_events
            .range((*user_id, previous_action, *event_type)..=(*user_id, *event_time, *event_type)),
    };
    let mut user_topics = user_view.remove(user_id).unwrap_or_else(HashMap::new);
    let found_score = user_topics.remove(topic);
    let mut user_for_update = match found_score {
        Some((date, score)) => {
            let old_user = ScoredUser {
                user_id: *user_id,
                score,
                last_action: date,
            };
            let topic_scores = topic_view
                .get_mut(topic)
                .ok_or_else(|| Error::Missing(format!("Topic {topic:?} from topic view")))?;
            let removed = topic_scores.remove(&old_user);
            if !removed {
                return Err(Error::Missing(format!(
                    "User with score for topic view {topic:?}"
                )));
            }
            old_user
        }
        None => default_user,
    };
    user_for_update.update_score(events_iter);
    user_topics.insert(
        topic.clone(),
        (user_for_update.last_action, user_for_update.score),
    );
    user_view.insert(*user_id, user_topics);
    match topic_view.get_mut(topic) {
        Some(topic_scores) => {
            topic_scores.insert(user_for_update);
        }
        None => {
            println!("Missing view for topic: {topic:?}");
            topic_view.insert(
                topic.clone(),
                once(user_for_update).collect::<BTreeSet<_>>(),
            );
        }
    }
    Ok(())
}
