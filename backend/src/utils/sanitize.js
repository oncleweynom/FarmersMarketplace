/**
 * Strip HTML tags and encode common XSS vectors from a string.
 * @param {string} text
 * @returns {string}
 */
function sanitizeText(text) {
  if (typeof text !== 'string') return '';
  return text
    .replace(/<[^>]*>/g, '')           // strip HTML tags
    .replace(/&/g, '&amp;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#x27;')
    .replace(/\//g, '&#x2F;')
    .trim();
const sanitizeHtml = require('sanitize-html');

// Strip all HTML tags — returns plain text only.
// Use this before storing any user-generated text in the DB.
function sanitizeText(value) {
  if (typeof value !== 'string') return value;
  return sanitizeHtml(value, { allowedTags: [], allowedAttributes: {} }).trim();
}

module.exports = { sanitizeText };
