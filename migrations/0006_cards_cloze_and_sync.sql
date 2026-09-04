alter table exams add column banca varchar(200);
-- Cards: organização, hierarquia, Cloze e metadados de sincronização
alter table cards add column position integer not null default 0;
alter table cards add column parent_card_id uuid references cards(id) on delete cascade;
alter table cards add column cloze_index integer;
alter table note_types add column is_cloze boolean not null default false;

create index cards_deck_position_idx on cards(deck_id, parent_card_id, position, inserted_at);
create index cards_parent_idx on cards(parent_card_id);
