import React from 'react';

const shimmer = `
@keyframes shimmer {
  0%   { background-position: -400px 0; }
  100% { background-position: 400px 0; }
}
`;

const base = {
  background: 'linear-gradient(90deg, #f0f0f0 25%, #e0e0e0 50%, #f0f0f0 75%)',
  backgroundSize: '800px 100%',
  animation: 'shimmer 1.4s infinite linear',
  borderRadius: 6,
};

export default function SkeletonProductCard() {
  return (
    <>
      <style>{shimmer}</style>
      <div style={{
        background: '#fff',
        borderRadius: 12,
        padding: 20,
        boxShadow: '0 1px 8px #0001',
        border: '2px solid transparent',
      }}>
        {/* image area */}
        <div style={{ ...base, width: '100%', height: 140, borderRadius: 8, marginBottom: 10 }} />
        {/* badge */}
        <div style={{ ...base, width: 60, height: 18, marginBottom: 8 }} />
        {/* name */}
        <div style={{ ...base, width: '80%', height: 18, marginBottom: 8 }} />
        {/* description line 1 */}
        <div style={{ ...base, width: '100%', height: 13, marginBottom: 4 }} />
        {/* description line 2 */}
        <div style={{ ...base, width: '65%', height: 13, marginBottom: 12 }} />
        {/* price */}
        <div style={{ ...base, width: 90, height: 22, marginBottom: 6 }} />
        {/* qty */}
        <div style={{ ...base, width: 70, height: 12, marginBottom: 10 }} />
        {/* compare btn */}
        <div style={{ ...base, width: 100, height: 32, borderRadius: 999, marginBottom: 10 }} />
        {/* view btn */}
        <div style={{ ...base, width: '100%', height: 36, borderRadius: 8, marginBottom: 12 }} />
        {/* seller section */}
        <div style={{ display: 'flex', gap: 10, alignItems: 'center', paddingTop: 12, borderTop: '1px solid #eee' }}>
          <div style={{ ...base, width: 36, height: 36, borderRadius: '50%', flexShrink: 0 }} />
          <div style={{ flex: 1 }}>
            <div style={{ ...base, width: '60%', height: 13, marginBottom: 4 }} />
            <div style={{ ...base, width: '40%', height: 11 }} />
          </div>
        </div>
      </div>
    </>
  );
}
