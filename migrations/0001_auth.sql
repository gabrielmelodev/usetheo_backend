create extension if not exists "pgcrypto";

create table users (
    id uuid primary key default gen_random_uuid(),
    first_name varchar(100) not null,
    last_name varchar(100) not null,
    email varchar(255) not null unique,
    cpf varchar(14) not null unique,
    country varchar(100) not null,
    city varchar(100) not null,
    password_hash text not null,
    email_verified_at timestamptz,
    must_change_password boolean not null default false,
    totp_secret text,
    totp_enabled boolean not null default false,
    role varchar(20) not null default 'user',
    inserted_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create table email_verification_tokens (
    id uuid primary key default gen_random_uuid(),
    user_id uuid not null references users(id) on delete cascade,
    token_hash text not null unique,
    expires_at timestamptz not null,
    consumed_at timestamptz,
    inserted_at timestamptz not null default now()
);

create table password_reset_tokens (
    id uuid primary key default gen_random_uuid(),
    user_id uuid not null references users(id) on delete cascade,
    token_hash text not null unique,
    expires_at timestamptz not null,
    consumed_at timestamptz,
    inserted_at timestamptz not null default now()
);

create table sessions (
    id uuid primary key default gen_random_uuid(),
    user_id uuid not null references users(id) on delete cascade,
    refresh_token_hash text not null unique,
    device_label varchar(255),
    user_agent text,
    ip_address varchar(64),
    last_used_at timestamptz not null default now(),
    revoked_at timestamptz,
    expires_at timestamptz not null,
    inserted_at timestamptz not null default now()
);

create index sessions_user_id_idx on sessions(user_id);

create table audit_logs (
    id uuid primary key default gen_random_uuid(),
    user_id uuid references users(id) on delete set null,
    action varchar(100) not null,
    ip_address varchar(64),
    user_agent text,
    metadata jsonb not null default '{}'::jsonb,
    inserted_at timestamptz not null default now()
);

create index audit_logs_user_id_idx on audit_logs(user_id);

create table login_attempts (
    id uuid primary key default gen_random_uuid(),
    email varchar(255) not null,
    ip_address varchar(64) not null,
    success boolean not null,
    inserted_at timestamptz not null default now()
);

create index login_attempts_lookup_idx on login_attempts(email, inserted_at);
