-- "Matérias": decks curados por administradores, publicados no catálogo para
-- que qualquer usuário clone uma cópia própria e independente para seus baralhos.
alter table decks add column is_official boolean not null default false;

create index decks_is_official_idx on decks(is_official) where is_official = true;
