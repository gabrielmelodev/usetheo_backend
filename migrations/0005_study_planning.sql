-- Editais / planos de estudo

create table exams (
    id uuid primary key default gen_random_uuid(),
    user_id uuid not null references users(id) on delete cascade,

    -- "Cargo / Órgão / Ano"
    name varchar(300) not null,

    -- 'deadline' (Com prazo certo) | 'continuous' (Contínuo) | 'free' (Livre)
    plan_mode varchar(20) not null,

    exam_date date, -- obrigatório no modo deadline; opcional/nulo nos outros

    -- ---- config do modo "Com prazo certo" ----
    sprint_days integer, -- 3 a 13

    -- ---- config do modo "Contínuo" ----
    weekly_hours double precision,
    continuous_mode varchar(10), -- 'semanal' | 'ciclo'
    max_session_minutes integer, -- só usado no modo ciclo

    -- ---- config do modo "Livre" ----
    review_intervals_days integer[], -- escada customizada, ex: {1,7,30}
    excluded_weekdays integer[] not null default '{}', -- 0=domingo .. 6=sábado

    inserted_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create index exams_user_id_idx on exams(user_id);

create table exam_subjects (
    id uuid primary key default gen_random_uuid(),
    exam_id uuid not null references exams(id) on delete cascade,
    name varchar(200) not null,
    weight integer not null default 3, -- 1 a 5
    knowledge_factor double precision not null default 1.0, -- "o quanto você já conhece"
    position integer not null default 0,

    inserted_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create index exam_subjects_exam_id_idx on exam_subjects(exam_id);

create table exam_topics (
    id uuid primary key default gen_random_uuid(),
    exam_id uuid not null references exams(id) on delete cascade,
    subject_id uuid not null references exam_subjects(id) on delete cascade,
    parent_topic_id uuid references exam_topics(id) on delete cascade,

    -- numeração hierárquica oficial do edital, ex: "1", "1.1", "1.2", "2"
    number varchar(20) not null,
    title text not null,
    position integer not null default 0,

    -- 1 a 4, usado tanto na distribuição quanto na priorização do sprint
    relevance integer not null default 2,

    -- true = este tópico entra no sprint final (modo deadline)
    in_sprint boolean not null default false,

    -- dia em que o cronograma automático agendou este tópico (deadline/contínuo-semanal)
    scheduled_date date,

    inserted_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create index exam_topics_exam_id_idx on exam_topics(exam_id);
create index exam_topics_subject_id_idx on exam_topics(subject_id);

-- Registro de estudo: o único lugar em que o usuário digita algo.
create table study_logs (
    id uuid primary key default gen_random_uuid(),
    user_id uuid not null references users(id) on delete cascade,
    exam_id uuid not null references exams(id) on delete cascade,
    topic_id uuid not null references exam_topics(id) on delete cascade,

    -- 'teoria' | 'revisao' | 'questoes' | 'videoaula' | 'leitura'
    category varchar(20) not null default 'teoria',

    studied_on date not null, -- nunca no futuro (checado na aplicação)
    minutes integer not null default 0,
    pages integer,
    video_minutes integer,
    questions_total integer,
    questions_correct integer,
    notes text,

    -- true quando o registro foi lançado no próprio dia (conta para constância)
    logged_same_day boolean not null default true,

    inserted_at timestamptz not null default now()
);

create index study_logs_user_id_idx on study_logs(user_id, studied_on);
create index study_logs_topic_id_idx on study_logs(topic_id);

-- Escada de revisão espaçada, gerada a partir dos registros (não é preenchida à mão).
create table topic_reviews (
    id uuid primary key default gen_random_uuid(),
    user_id uuid not null references users(id) on delete cascade,
    topic_id uuid not null references exam_topics(id) on delete cascade,
    study_log_id uuid references study_logs(id) on delete set null,

    step integer not null, -- posição na escada (1, 2, 3...), recomeça após o último degrau
    scheduled_date date not null,
    completed_at timestamptz,
    completed_study_log_id uuid references study_logs(id) on delete set null,
    skipped boolean not null default false,

    inserted_at timestamptz not null default now()
);

create index topic_reviews_user_id_idx on topic_reviews(user_id, scheduled_date);
create index topic_reviews_topic_id_idx on topic_reviews(topic_id);

-- Plano "Livre": grade semanal montada à mão (tudo opcional).
create table manual_schedule_entries (
    id uuid primary key default gen_random_uuid(),
    user_id uuid not null references users(id) on delete cascade,
    exam_id uuid not null references exams(id) on delete cascade,
    weekday integer not null, -- 0=domingo .. 6=sábado
    subject_id uuid references exam_subjects(id) on delete set null,
    topic_id uuid references exam_topics(id) on delete set null,
    planned_minutes integer,
    question_goal integer,

    inserted_at timestamptz not null default now()
);

create index manual_schedule_entries_exam_id_idx on manual_schedule_entries(exam_id);
