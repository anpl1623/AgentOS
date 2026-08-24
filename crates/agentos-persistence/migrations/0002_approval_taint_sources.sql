-- Record where an approval request's untrusted data came from.
--
-- Previously the runtime composed a sentence about this and appended it to the
-- explanation, which meant a presentation decision was baked into a domain
-- object and every client had to accept the same wording. The sources travel as
-- data now; each client writes its own sentence.
ALTER TABLE approvals ADD COLUMN taint_sources TEXT NOT NULL DEFAULT '[]';
