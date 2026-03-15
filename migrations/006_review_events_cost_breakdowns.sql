-- Add workload/role/provider/model cost breakdown rows per review event.
ALTER TABLE review_events
ADD COLUMN IF NOT EXISTS cost_breakdowns JSONB NOT NULL DEFAULT '[]';
