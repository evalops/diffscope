-- Add server-side cost estimate (USD) per review event for stats aggregation.
ALTER TABLE review_events ADD COLUMN IF NOT EXISTS cost_estimate_usd REAL;
