const router = require('express').Router();
const db = require('../db/schema');
const auth = require('../middleware/auth');
const { err } = require('../middleware/error');
const { sanitizeText } = require('../utils/sanitize');

// Schema migration for categories
db.exec(`
  CREATE TABLE IF NOT EXISTS categories (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    slug TEXT UNIQUE NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
  );
`);
try { db.exec(`ALTER TABLE products ADD COLUMN category_id INTEGER REFERENCES categories(id)`); } catch {}

function toSlug(name) {
  return name.toLowerCase().trim().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
}

// GET /api/categories - public, includes product_count
router.get('/', (_req, res) => {
  const rows = db.prepare(`
    SELECT c.*, COUNT(p.id) as product_count
    FROM categories c
    LEFT JOIN products p ON p.category_id = c.id
    GROUP BY c.id
    ORDER BY c.name
  `).all();
  res.json({ success: true, data: rows });
});

// POST /api/admin/categories - admin only
router.post('/admin/categories', auth, (req, res) => {
  if (req.user.role !== 'admin') return err(res, 403, 'Admins only', 'forbidden');

  const name = sanitizeText(req.body.name || '');
  if (!name) return err(res, 400, 'name is required', 'validation_error');

  const slug = sanitizeText(req.body.slug || '') || toSlug(name);
  if (!/^[a-z0-9-]+$/.test(slug))
    return err(res, 400, 'slug must be lowercase alphanumeric with hyphens', 'validation_error');

  const existing = db.prepare('SELECT id FROM categories WHERE slug = ?').get(slug);
  if (existing) return err(res, 409, 'A category with that slug already exists', 'slug_taken');

  const result = db.prepare('INSERT INTO categories (name, slug) VALUES (?, ?)').run(name, slug);
  res.status(201).json({ success: true, id: result.lastInsertRowid, name, slug });
});

// PATCH /api/admin/categories/:id - admin only
router.patch('/admin/categories/:id', auth, (req, res) => {
  if (req.user.role !== 'admin') return err(res, 403, 'Admins only', 'forbidden');

  const category = db.prepare('SELECT * FROM categories WHERE id = ?').get(req.params.id);
  if (!category) return err(res, 404, 'Category not found', 'not_found');

  const name = req.body.name ? sanitizeText(req.body.name) : category.name;
  let slug = req.body.slug ? sanitizeText(req.body.slug) : (req.body.name ? toSlug(name) : category.slug);

  if (slug !== category.slug) {
    if (!/^[a-z0-9-]+$/.test(slug))
      return err(res, 400, 'slug must be lowercase alphanumeric with hyphens', 'validation_error');
    const conflict = db.prepare('SELECT id FROM categories WHERE slug = ? AND id != ?').get(slug, req.params.id);
    if (conflict) return err(res, 409, 'A category with that slug already exists', 'slug_taken');
  }

  db.prepare('UPDATE categories SET name = ?, slug = ? WHERE id = ?').run(name, slug, req.params.id);
  res.json({ success: true, id: Number(req.params.id), name, slug });
});

// DELETE /api/admin/categories/:id - admin only
router.delete('/admin/categories/:id', auth, (req, res) => {
  if (req.user.role !== 'admin') return err(res, 403, 'Admins only', 'forbidden');

  const category = db.prepare('SELECT * FROM categories WHERE id = ?').get(req.params.id);
  if (!category) return err(res, 404, 'Category not found', 'not_found');

  const productCount = db.prepare('SELECT COUNT(*) as count FROM products WHERE category_id = ?').get(req.params.id).count;
  if (productCount > 0)
    return err(res, 409, 'Cannot delete category with assigned products', 'category_has_products');

  db.prepare('DELETE FROM categories WHERE id = ?').run(req.params.id);
  res.json({ success: true, message: 'Category deleted' });
const db = require('../db/postgres');

router.get('/', async (req, res) => {
  const result = await db.query('SELECT * FROM categories ORDER BY name');
  res.json(result.rows);
});

module.exports = router;
