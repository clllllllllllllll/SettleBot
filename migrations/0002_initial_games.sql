INSERT INTO games (id, display_name) VALUES ('mahjong', 'Mahjong'), ('poker', 'Poker')
    ON CONFLICT(id) DO NOTHING;
