/**
 * DOM utility functions
 */

/**
 * Escape HTML to prevent XSS
 * @param {string} text - Text to escape
 * @returns {string} Escaped HTML
 */
export function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

/**
 * Create an HTML element with attributes and children
 * @param {string} tag - HTML tag name
 * @param {Object} attrs - Attributes to set
 * @param {Array} children - Child elements or text
 * @returns {HTMLElement} The created element
 */
export function createElement(tag, attrs = {}, children = []) {
    const element = document.createElement(tag);
    
    for (const [key, value] of Object.entries(attrs)) {
        if (key === 'className') {
            element.className = value;
        } else if (key === 'dataset') {
            Object.assign(element.dataset, value);
        } else if (key.startsWith('on')) {
            element.addEventListener(key.slice(2).toLowerCase(), value);
        } else {
            element.setAttribute(key, value);
        }
    }
    
    for (const child of children) {
        if (typeof child === 'string') {
            element.appendChild(document.createTextNode(child));
        } else if (child instanceof Node) {
            element.appendChild(child);
        }
    }
    
    return element;
}

/**
 * Query selector with error handling
 * @param {string} selector - CSS selector
 * @param {Element} parent - Parent element to search in
 * @returns {Element|null} The found element or null
 */
export function $(selector, parent = document) {
    try {
        return parent.querySelector(selector);
    } catch (e) {
        console.error('Invalid selector:', selector);
        return null;
    }
}

/**
 * Query selector all with error handling
 * @param {string} selector - CSS selector
 * @param {Element} parent - Parent element to search in
 * @returns {NodeList} The found elements
 */
export function $$(selector, parent = document) {
    try {
        return parent.querySelectorAll(selector);
    } catch (e) {
        console.error('Invalid selector:', selector);
        return [];
    }
}