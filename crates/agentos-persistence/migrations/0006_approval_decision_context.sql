-- What a person needed to know when they decided, kept with what they decided.
--
-- effect_before_taint is what the policy alone said. 'allow' there means the
-- request exists only because the run had read untrusted data, which is the
-- most decision-relevant fact a card can state. Rows written before this
-- column did not record it; they read as 'ask', the effect they were raised
-- under, which claims the policy asked and never that taint alone did.
ALTER TABLE approvals ADD COLUMN effect_before_taint TEXT NOT NULL DEFAULT 'ask';

-- Which request this was in its run, counting from one, and the budget the
-- run's policy set. Zero and NULL for rows written before the budget existed:
-- nothing counted them, and inventing a count would be worse than none.
ALTER TABLE approvals ADD COLUMN asked_this_run INTEGER NOT NULL DEFAULT 0;
ALTER TABLE approvals ADD COLUMN approval_budget INTEGER;

-- Decided requests are read newest decision first. A partial expression
-- index, so the history does not cost a scan of every request the
-- installation has ever raised, and never holds the requests still waiting,
-- which belong to the queue rather than the history. The COALESCE covers any
-- row that was closed without a decision time being written.
CREATE INDEX idx_approvals_recent ON approvals(COALESCE(decided_at, requested_at) DESC)
    WHERE status <> 'pending';
