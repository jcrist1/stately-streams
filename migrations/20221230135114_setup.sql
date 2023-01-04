-- Add migration script here
create table users (
  id bigserial primary key,
  username varchar(255) not null,
  password varchar(255) not null,
  created timestamptz not null,
  updated timestamptz not null,
  deleted timestamptz
);

create table user_actions (
  id bigserial primary key,
  user_id bigint not null, -- fk
  action_type varchar(255) not null,
  action_date timestamptz not null
);

create table user_action_topics (
  id bigserial primary key,
  user_action bigint not null, -- fk 
  topic varchar(255) not null -- unique key (action, action)
);

alter table user_actions add constraint user_action_user foreign key (user_id) references users (id);

alter table user_action_topics add constraint user_action_topics_action foreign key (user_action) references user_actions (id);

alter table user_action_topics add constraint unique_topic_per_action unique (user_action, topic);

create index actions_by_topic on user_action_topics (topic);

