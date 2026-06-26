import React, { useState } from 'react';
import { api } from '../../api/client';
import { getErrorMessage } from '../../utils/errorMessages';

const s = {
  label: { display: 'block', fontSize: 13, marginBottom: 4, color: '#555' },
  input: { width: '100%', padding: '9px 12px', border: '1px solid #ddd', borderRadius: 8, fontSize: 16, marginBottom: 4, boxSizing: 'border-box', minHeight: 44 },
  btn: { background: '#2d6a4f', color: '#fff', border: 'none', borderRadius: 8, padding: '10px 20px', cursor: 'pointer', fontWeight: 600, minHeight: 44 },
  msg: { padding: '10px 14px', borderRadius: 8, marginBottom: 12, fontSize: 14 },
};

export default function FlashSaleManager({ products, onChanged }) {
  const [form, setForm] = useState({ product_id: '', flash_sale_price: '', flash_sale_ends_at: '' });
  const [msg, setMsg] = useState(null);

  async function handleSubmit(e) {
    e.preventDefault();
    setMsg(null);
    try {
      const res = await api.setFlashSale(parseInt(form.product_id, 10), {
        flash_sale_price: parseFloat(form.flash_sale_price),
        flash_sale_ends_at: new Date(form.flash_sale_ends_at).toISOString(),
      });
      setMsg({ type: 'ok', text: `Flash sale set for product #${res.data.id}` });
      onChanged?.();
    } catch (e) {
      setMsg({ type: 'err', text: getErrorMessage(e) });
    }
  }

  async function handleCancel(productId) {
    try {
      await api.cancelFlashSale(productId);
      setMsg({ type: 'ok', text: `Flash sale canceled for product #${productId}` });
      onChanged?.();
    } catch (e) {
      setMsg({ type: 'err', text: getErrorMessage(e) });
    }
  }

  return (
    <div style={{ background: '#fff', borderRadius: 12, padding: 24, boxShadow: '0 1px 8px #0001', marginBottom: 24 }}>
      <h3 style={{ marginBottom: 12, color: '#333' }}>Flash Sales</h3>
      {msg && (
        <div style={{ ...s.msg, background: msg.type === 'ok' ? '#d8f3dc' : '#fee', color: msg.type === 'ok' ? '#2d6a4f' : '#c0392b' }}>
          {msg.text}
        </div>
      )}
      <form onSubmit={handleSubmit} style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr auto', gap: 10, alignItems: 'end' }}>
        <div>
          <label style={s.label}>Product</label>
          <select style={s.input} value={form.product_id} onChange={e => setForm(f => ({ ...f, product_id: e.target.value }))} required>
            <option value="">Select product</option>
            {products.map(p => <option key={p.id} value={p.id}>{p.name}</option>)}
          </select>
        </div>
        <div>
          <label style={s.label}>Flash Price (XLM)</label>
          <input style={s.input} type="number" min="0" step="any" required value={form.flash_sale_price} onChange={e => setForm(f => ({ ...f, flash_sale_price: e.target.value }))} />
        </div>
        <div>
          <label style={s.label}>Ends At</label>
          <input style={s.input} type="datetime-local" required value={form.flash_sale_ends_at} onChange={e => setForm(f => ({ ...f, flash_sale_ends_at: e.target.value }))} />
        </div>
        <button type="submit" style={s.btn}>Set Flash Sale</button>
      </form>
      <div style={{ marginTop: 14 }}>
        {products.filter(p => p.flash_sale_price && p.flash_sale_ends_at).map(p => (
          <div key={p.id} style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', borderTop: '1px solid #eee', paddingTop: 10, marginTop: 10 }}>
            <div style={{ fontSize: 14 }}>
              <strong>{p.name}</strong> – {p.flash_sale_price} XLM until {new Date(p.flash_sale_ends_at).toLocaleString()}
            </div>
            <button type="button" style={{ ...s.btn, background: '#c0392b' }} onClick={() => handleCancel(p.id)}>Cancel</button>
          </div>
        ))}
      </div>
    </div>
  );
}
