ALTER TABLE rates
ADD CONSTRAINT rates_type_check CHECK (type IN ('post', 'story', 'reel'));
