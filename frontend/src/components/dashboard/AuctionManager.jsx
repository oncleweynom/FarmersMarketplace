import React, { useState } from 'react';
import { api } from '../../api/client';

const s = {
  label: { display: 'block', fontSize: 13, marginBottom: 4, color: '#555' },
  input: { width: '100%', padding: '9px 12px', border: '1px solid #ddd', borderRadius: 8, fontSize: 16, marginBottom: 4, boxSizing: 'border-box', minHeight: 44 },
  btn: { background: '#2d6a4f', color: '#fff', border: 'none', borderRadius: 8, padding: '10px 20px', cursor: 'pointer', fontWeight: 600, minHeight: 44 },
  msg: { padding: '10px 14px', borderRadius: 8, marginBottom: 12, fontSize: 14 },
};

export default function AuctionManager({ products }) {
  const [form, setForm] = useState({ product_id: '', start_price: '', ends_at: '' });
  const [msg, setMsg] = useState(null);

  async function handleSubmit(e) {
    e.preventDefault();
    setMsg(null);
    try {
      await api.createAuction({
        product_id: parseInt(form.product_id),
        start_price: parseFloat(form.start_price),
        ends_at: new Date(form.ends_at).toISOString(),
      });
      setMsg({ type: 'ok', text: 'Auction created!' });
      setForm({ product_id: '', start_price: '', ends_at: '' });
    } catch (err) {
      setMsg({ type: 'err', text: err.message });
    }
  }

  return (
    <div style={{ background: '#fff', borderRadius: 12, padding: 24, boxShadow: '0 1px 8px #0001', marginTop: 24, maxWidth: 440 }}>
      <h3 style={{ marginBottom: 16, color: '#333' }}>🔨 Create Auction</h3>
      {msg && (
        <div style={{ ...s.msg, background: msg.type === 'ok' ? '#d8f3dc' : '#fee', color: msg.type === 'ok' ? '#2d6a4f' : '#c0392b' }}>
          {msg.text}
        </div>
      )}
      <form onSubmit={handleSubmit}>
        <label style={s.label}>Product</label>
        <select style={s.input} value={form.product_id} onChange={e => setForm({ ...form, product_id: e.target.value })} required>
          <option value="">Select a product</option>
          {products.map(p => <option key={p.id} value={p.id}>{p.name}</option>)}
        </select>
        <label style={s.label}>Starting Price (XLM)</label>
        <input style={s.input} type="number" min="0.01" step="0.01" value={form.start_price} onChange={e => setForm({ ...form, start_price: e.target.value })} required />
        <label style={s.label}>Ends At</label>
        <input style={s.input} type="datetime-local" value={form.ends_at} onChange={e => setForm({ ...form, ends_at: e.target.value })} required />
        <button style={{ ...s.btn, background: '#e07b00' }} type="submit">Create Auction</button>
      </form>
    </div>
  );
}
