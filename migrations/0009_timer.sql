-- Study timer
-- Stores the current timer state for each user/session.

create table study_timers (
    user_id uuid primary key references users(id) on delete cascade,
    exam_id uuid not null references exams(id) on delete cascade,
    topic_id uuid not null references exam_topics(id) on delete cascade,
    session_id uuid not null,
    elapsed_seconds bigint not null default 0,
    running boolean not null default false,
    started_at timestamptz,
    updated_at timestamptz not null default now(),
    category text not null default 'teoria',
    running_since timestamptz
);

create index study_timers_exam_id_idx
    on study_timers(exam_id);

create index study_timers_topic_id_idx
    on study_timers(topic_id);

create index study_timers_session_id_idx
    on study_timers(session_id);