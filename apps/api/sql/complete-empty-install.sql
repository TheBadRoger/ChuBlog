-- Deployment stops the API first; run after confirming both installation tables are empty.
-- accounts.sql then creates both tables with the current schema.
DROP TABLE blog_site;
DROP TABLE blog_users;
