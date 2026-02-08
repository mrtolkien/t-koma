-- Rename note_type → entry_type and add archetype column.
--
-- entry_type: structural discrimination (Note, ReferenceTopic, ReferenceCollection,
--             ReferenceDocs, ReferenceCode, Diary). Used in WHERE clauses.
-- archetype:  optional semantic classification for notes (person, concept, decision, ...).
--             NULL for non-note entry types (references, diary).

-- Step 1: Rename column and add archetype
ALTER TABLE notes RENAME COLUMN note_type TO entry_type;
ALTER TABLE notes ADD COLUMN archetype TEXT;

-- Step 2: Migrate existing notes — move semantic types into archetype, set entry_type = 'Note'
UPDATE notes SET archetype = LOWER(entry_type), entry_type = 'Note'
    WHERE scope IN ('shared_note', 'ghost_note');

-- Step 3: Drop the obsolete type_valid column
ALTER TABLE notes DROP COLUMN type_valid;

-- Step 4: Rebuild chunk_fts with entry_type replacing note_type, and archetype added
DROP TABLE IF EXISTS chunk_fts;
CREATE VIRTUAL TABLE IF NOT EXISTS chunk_fts USING fts5(
    content,
    title,
    note_title,
    entry_type,
    archetype,
    note_id UNINDEXED,
    chunk_id UNINDEXED
);

-- Step 5: Repopulate FTS from chunks + notes
INSERT INTO chunk_fts (content, title, note_title, entry_type, archetype, note_id, chunk_id)
    SELECT c.content, c.title, n.title, n.entry_type, n.archetype, c.note_id, c.id
    FROM chunks c
    JOIN notes n ON n.id = c.note_id;
