-- Add missing columns to admins table
ALTER TABLE admins 
ADD COLUMN steam_id_3 VARCHAR(64),
ADD COLUMN steam_id_64 VARCHAR(64);
