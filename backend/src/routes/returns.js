const router = require('express').Router();
const db = require('../db/schema');
const auth = require('../middleware/auth');

// POST /api/returns - buyer submits a return request
router.post('/', auth, (req, res) => {
  if (req.user.role !== 'buyer')
    return res.status(403).json({ error: 'Only buyers can request returns' });

  const { order_id, reason } = req.body;
  if (!order_id || !reason)
    return res.status(400).json({ error: 'order_id and reason required' });

  const order = db.prepare('SELECT * FROM orders WHERE id = ? AND buyer_id = ?')
    .get(order_id, req.user.id);
  if (!order) return res.status(404).json({ error: 'Order not found' });
  if (order.status !== 'paid')
    return res.status(400).json({ error: 'Only paid orders can be returned' });

  const existing = db.prepare('SELECT id FROM returns WHERE order_id = ?').get(order_id);
  if (existing)
    return res.status(409).json({ error: 'Return request already submitted for this order' });

  const result = db.prepare(
    'INSERT INTO returns (order_id, buyer_id, reason) VALUES (?, ?, ?)'
  ).run(order_id, req.user.id, reason);

  res.status(201).json({ id: result.lastInsertRowid, message: 'Return request submitted' });
});

// GET /api/returns - buyer's own return requests
router.get('/', auth, (req, res) => {
  if (req.user.role !== 'buyer')
    return res.status(403).json({ error: 'Only buyers can view their returns' });

  const returns = db.prepare(`
    SELECT r.*, p.name AS product_name, o.total_price, o.shipping_cost, o.quantity
    FROM returns r
    JOIN orders o ON r.order_id = o.id
    JOIN products p ON o.product_id = p.id
    WHERE r.buyer_id = ?
    ORDER BY r.created_at DESC
  `).all(req.user.id);
  res.json(returns);
const router = require('express').Router({ mergeParams: true });
const db = require('../db/schema');
const auth = require('../middleware/auth');

// POST /api/returns - buyer submits a return request
router.post('/', auth, (req, res) => {
  if (req.user.role !== 'buyer')
    return res.status(403).json({ error: 'Only buyers can request returns' });

  const { order_id, reason } = req.body;
  if (!order_id || !reason)
    return res.status(400).json({ error: 'order_id and reason required' });

  const order = db.prepare('SELECT * FROM orders WHERE id = ? AND buyer_id = ?')
    .get(order_id, req.user.id);
  if (!order) return res.status(404).json({ error: 'Order not found' });
  if (order.status !== 'paid')
    return res.status(400).json({ error: 'Only paid orders can be returned' });

  const existing = db.prepare('SELECT id FROM returns WHERE order_id = ?').get(order_id);
  if (existing)
    return res.status(409).json({ error: 'Return request already submitted for this order' });

  const result = db.prepare(
    'INSERT INTO returns (order_id, buyer_id, reason) VALUES (?, ?, ?)'
  ).run(order_id, req.user.id, reason);

  res.status(201).json({ id: result.lastInsertRowid, message: 'Return request submitted' });
});

// GET /api/returns - buyer's own return requests
router.get('/', auth, (req, res) => {
  if (req.user.role !== 'buyer')
    return res.status(403).json({ error: 'Only buyers can view their returns' });

  const returns = db.prepare(`
    SELECT r.*, p.name AS product_name, o.total_price, o.shipping_cost, o.quantity
    FROM returns r
    JOIN orders o ON r.order_id = o.id
    JOIN products p ON o.product_id = p.id
    WHERE r.buyer_id = ?
    ORDER BY r.created_at DESC
  `).all(req.user.id);
  res.json(returns);
});

module.exports = router;
