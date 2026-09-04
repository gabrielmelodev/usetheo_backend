create table folders (
    id uuid primary key default gen_random_uuid(),
    user_id uuid not null references users(id) on delete cascade,
    parent_folder_id uuid references folders(id) on delete set null,
    name varchar(200) not null,
    inserted_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create index folders_user_id_idx on folders(user_id);

create table decks (
    id uuid primary key default gen_random_uuid(),
    user_id uuid not null references users(id) on delete cascade,
    folder_id uuid references folders(id) on delete set null,
    parent_deck_id uuid references decks(id) on delete set null,

    name varchar(200) not null,
    description text,
    support_text text,
    about text,

    target_cards integer not null default 20,
    card_type varchar(30) not null default 'basic',
    difficulty varchar(20) not null default 'medio',
    color varchar(20) not null default '#8b5cf6',
    icon varchar(50),

    show_review_queue boolean not null default true,
    allow_subdecks boolean not null default true,
    is_public boolean not null default false,

    anki_name varchar(255),
    imported_from_anki boolean not null default false,
    review_cards_per_day integer,

    inserted_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create index decks_user_id_idx on decks(user_id);
create index decks_folder_id_idx on decks(folder_id);

create table note_types (
    id uuid primary key default gen_random_uuid(),
    user_id uuid not null references users(id) on delete cascade,
    name varchar(200) not null,
    fields jsonb not null default '[]'::jsonb,
    templates jsonb not null default '[]'::jsonb,
    inserted_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create index note_types_user_id_idx on note_types(user_id);

create table notes (
    id uuid primary key default gen_random_uuid(),
    user_id uuid not null references users(id) on delete cascade,
    deck_id uuid not null references decks(id) on delete cascade,
    note_type_id uuid not null references note_types(id) on delete restrict,
    fields jsonb not null default '{}'::jsonb,
    tags text[] not null default '{}',
    inserted_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create index notes_deck_id_idx on notes(deck_id);
create index notes_user_id_idx on notes(user_id);

create table cards (
    id uuid primary key default gen_random_uuid(),
    user_id uuid not null references users(id) on delete cascade,
    note_id uuid not null references notes(id) on delete cascade,
    deck_id uuid not null references decks(id) on delete cascade,

    template_index integer not null default 0,
    queue varchar(20) not null default 'new',
    due timestamptz not null default now(),
    interval_days double precision not null default 0,
    ease_factor double precision not null default 2.5,
    repetitions integer not null default 0,
    lapses integer not null default 0,

    inserted_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create index cards_deck_id_idx on cards(deck_id);
create index cards_due_idx on cards(user_id, queue, due);

create table review_logs (
    id uuid primary key default gen_random_uuid(),
    card_id uuid not null references cards(id) on delete cascade,
    user_id uuid not null references users(id) on delete cascade,
    rating varchar(10) not null,
    interval_before double precision not null,
    interval_after double precision not null,
    ease_before double precision not null,
    ease_after double precision not null,
    reviewed_at timestamptz not null default now()
);

create index review_logs_user_id_idx on review_logs(user_id, reviewed_at);
