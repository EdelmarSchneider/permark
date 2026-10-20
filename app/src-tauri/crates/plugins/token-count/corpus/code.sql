-- Orders per customer in the last 30 days
SELECT c.id,
       c.name,
       COUNT(o.id)            AS orders,
       SUM(o.total_cents)/100 AS total
FROM customers AS c
LEFT JOIN orders AS o
       ON o.customer_id = c.id
      AND o.created_at >= NOW() - INTERVAL '30 days'
WHERE c.active = TRUE
GROUP BY c.id, c.name
HAVING COUNT(o.id) > 0
ORDER BY total DESC
LIMIT 25;
