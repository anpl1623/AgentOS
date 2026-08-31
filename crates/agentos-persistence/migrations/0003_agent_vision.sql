-- Whether an agent's model can be shown images.
--
-- Nullable on purpose. NULL means "take the provider's default", which is the
-- honest answer for every agent that existed before vision did, and which lets
-- an operator running a local vision model say so without the runtime having to
-- guess from a model name.
ALTER TABLE agents ADD COLUMN vision INTEGER;
