-- Baralhos modelo (admin) + organização em massa

alter table decks add column is_template boolean not null default false;
alter table decks add column position integer not null default 0;
alter table decks add column copied_from_deck_id uuid references decks(id) on delete set null;

alter table folders add column position integer not null default 0;

create index decks_is_template_idx on decks(is_template) where is_template = true;
