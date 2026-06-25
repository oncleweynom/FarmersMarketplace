const router = require('express').Router();
const db = require('../db/schema');
const auth = require('../middleware/auth');
const validate = require('../middleware/validate');
const { sendDisputeResolvedEmail } = require('../utils/mailer');
const { burnRewardTokens } = require('../utils/stellar');
const logger = require('../logger');

// POST /api/disputes — buyer files a dispute on a paid order
router.post('/', auth, validate.dispute, async (req, res, next) => {
  try {
    if (req.user.role !== 'buyer')
      return res.status(403).json({ error: 'Only buyers can file disputes' });

    const order_id = parseInt(req.body.order_id, 10);
    const { reason } = req.body;

    const { rows: orderRows } = await db.query(
      'SELECT * FROM orders WHERE id = $1 AND buyer_id = $2',
      [order_id, req.user.id]
    );
    const order = orderRows[0];

    if (!order) return res.status(404).json({ error: 'Order not found' });
    if (order.status !== 'paid')
      return res.status(400).json({ error: 'Disputes can only be filed on paid orders' });

    const { rows: existingRows } = await db.query(
      'SELECT id FROM disputes WHERE order_id = $1',
      [order_id]
    );
    if (existingRows[0])
      return res.status(409).json({ error: 'A dispute already exists for this order' });

    const { rows: inserted } = await db.query(
      'INSERT INTO disputes (order_id, buyer_id, reason) VALUES ($1, $2, $3) RETURNING id',
      [order_id, req.user.id, reason.trim()]
    );

    res.status(201).json({ id: inserted[0].id, order_id, status: 'open', message: 'Dispute filed' });
  } catch (err) {
    next(err);
  }
});

// GET /api/disputes — admin lists all disputes
router.get('/', auth, async (req, res, next) => {
  try {
    if (req.user.role !== 'admin')
      return res.status(403).json({ error: 'Admins only' });

    const { rows } = await db.query(`
      SELECT d.*, u.name as buyer_name, u.email as buyer_email,
             o.total_price, o.quantity, p.name as product_name
      FROM disputes d
      JOIN users u ON d.buyer_id = u.id
      JOIN orders o ON d.order_id = o.id
      JOIN products p ON o.product_id = p.id
      ORDER BY d.created_at DESC
    `);

    res.json(rows);
  } catch (err) {
    next(err);
  }
});

// PATCH /api/disputes/:id — admin resolves a dispute
router.patch('/:id', auth, validate.resolveDispute, async (req, res, next) => {
  try {
    if (req.user.role !== 'admin')
      return res.status(403).json({ error: 'Admins only' });

    const { rows: disputeRows } = await db.query(
      'SELECT * FROM disputes WHERE id = $1',
      [req.params.id]
    );
    const dispute = disputeRows[0];
    if (!dispute) return res.status(404).json({ error: 'Dispute not found' });

    const { status, resolution } = req.body;

    const transitions = { open: ['under_review'], under_review: ['resolved'], resolved: [] };
    if (!transitions[dispute.status].includes(status))
      return res.status(400).json({ error: `Cannot transition from '${dispute.status}' to '${status}'` });

    if (status === 'resolved' && (!resolution || !resolution.trim()))
      return res.status(400).json({ error: 'A resolution note is required when resolving a dispute' });

    await db.query(
      'UPDATE disputes SET status = $1, resolution = $2 WHERE id = $3',
      [status, resolution ? resolution.trim() : dispute.resolution, dispute.id]
    );

    if (status === 'resolved') {
      const [{ rows: buyerRows }, { rows: orderRows }] = await Promise.all([
        db.query('SELECT * FROM users WHERE id = $1', [dispute.buyer_id]),
        db.query('SELECT * FROM orders WHERE id = $1', [dispute.order_id]),
      ]);
      const buyer = buyerRows[0];
      const order = orderRows[0];
      const { rows: productRows } = await db.query(
        'SELECT * FROM products WHERE id = $1',
        [order.product_id]
      );

      sendDisputeResolvedEmail({
        dispute: { ...dispute, resolution: resolution.trim() },
        order,
        product: productRows[0],
        buyer,
      }).catch((e) => logger.error('Dispute email failed:', e.message));

      // #847 — burn reward tokens earned for this order (non-fatal)
      if (buyer?.stellar_public_key) {
        const burnAmount = Math.floor(Number(order.total_price));
        if (burnAmount > 0) {
          try {
            burnRewardTokens(buyer.stellar_public_key, burnAmount)
              .catch((e) => logger.warn('[Rewards] Burn failed on dispute resolve (non-fatal):', { error: e.message }));
          } catch (e) {
            logger.warn('[Rewards] Burn failed on dispute resolve (non-fatal):', { error: e.message });
          }
        }
      }
    }

    res.json({ id: dispute.id, status, message: 'Dispute updated' });
  } catch (err) {
    next(err);
  }
});

module.exports = router;
