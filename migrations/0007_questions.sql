-- Questions: exam questions with alternatives and metadata

create table questions (
    id uuid primary key default gen_random_uuid(),
    user_id uuid not null references users(id) on delete cascade,
    exam_id uuid references exams(id) on delete set null,
    subject_id uuid references exam_subjects(id) on delete set null,
    topic_id uuid references exam_topics(id) on delete set null,

    banca varchar(200),
    source varchar(255),
    year integer,

    question_type varchar(20) not null default 'multiple_choice',
    statement text not null,
    alternatives jsonb not null default '[]'::jsonb,
    correct_alternative_id varchar(255),
    explanation text,

    difficulty varchar(20) not null default 'medio',

    inserted_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create index questions_user_id_idx on questions(user_id);
create index questions_exam_id_idx on questions(exam_id);
create index questions_subject_id_idx on questions(subject_id);
create index questions_topic_id_idx on questions(topic_id);

-- Question attempts: tracks user attempts to answer questions

create table question_attempts (
    id uuid primary key default gen_random_uuid(),
    user_id uuid not null references users(id) on delete cascade,
    question_id uuid not null references questions(id) on delete cascade,
    selected_alternative_id varchar(255),
    is_correct boolean not null default false,
    answered_at timestamptz not null default now()
);

create index question_attempts_user_id_idx on question_attempts(user_id);
create index question_attempts_question_id_idx on question_attempts(question_id);
create index question_attempts_answered_at_idx on question_attempts(user_id, answered_at);