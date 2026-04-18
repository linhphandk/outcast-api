DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM rates
        WHERE type NOT IN ('post', 'story', 'reel')
    ) THEN
        RAISE EXCEPTION 'Cannot restore rates_type_check: rates.type contains values outside post/story/reel';
    END IF;
END;
$$;

ALTER TABLE rates
ADD CONSTRAINT rates_type_check CHECK (type IN ('post', 'story', 'reel'));
