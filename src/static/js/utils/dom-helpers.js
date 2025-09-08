// DOM manipulation utilities

/**
 * Query selector with optional parent
 * @param {string} selector - CSS selector
 * @param {Element} parent - Parent element (defaults to document)
 * @returns {Element|null} The matching element
 */
export function $(selector, parent = document) {
    return parent.querySelector(selector);
}

/**
 * Query selector all with optional parent
 * @param {string} selector - CSS selector
 * @param {Element} parent - Parent element (defaults to document)
 * @returns {NodeList} The matching elements
 */
export function $$(selector, parent = document) {
    return parent.querySelectorAll(selector);
}

/**
 * Create an element with optional attributes and children
 * @param {string} tag - The tag name
 * @param {Object} attrs - Attributes to set
 * @param {...(string|Element)} children - Child elements or text
 * @returns {Element} The created element
 */
export function createElement(tag, attrs = {}, ...children) {
    const element = document.createElement(tag);
    
    // Set attributes
    for (const [key, value] of Object.entries(attrs)) {
        if (key === 'className') {
            element.className = value;
        } else if (key === 'style' && typeof value === 'object') {
            Object.assign(element.style, value);
        } else if (key.startsWith('on')) {
            element.addEventListener(key.slice(2).toLowerCase(), value);
        } else {
            element.setAttribute(key, value);
        }
    }
    
    // Add children
    for (const child of children) {
        if (typeof child === 'string') {
            element.appendChild(document.createTextNode(child));
        } else if (child instanceof Element) {
            element.appendChild(child);
        }
    }
    
    return element;
}

/**
 * Remove all children from an element
 * @param {Element} element - The element to clear
 */
export function clearElement(element) {
    while (element.firstChild) {
        element.removeChild(element.firstChild);
    }
}

/**
 * Replace an element's children with new content
 * @param {Element} element - The element to update
 * @param {...(string|Element)} children - New children
 */
export function setChildren(element, ...children) {
    clearElement(element);
    for (const child of children) {
        if (typeof child === 'string') {
            element.appendChild(document.createTextNode(child));
        } else if (child instanceof Element) {
            element.appendChild(child);
        }
    }
}

/**
 * Add or remove a class based on a condition
 * @param {Element} element - The element to modify
 * @param {string} className - The class name
 * @param {boolean} condition - Whether to add or remove the class
 */
export function toggleClass(element, className, condition) {
    if (condition) {
        element.classList.add(className);
    } else {
        element.classList.remove(className);
    }
}

/**
 * Show an element
 * @param {Element} element - The element to show
 * @param {string} display - The display value (defaults to 'block')
 */
export function show(element, display = 'block') {
    element.style.display = display;
}

/**
 * Hide an element
 * @param {Element} element - The element to hide
 */
export function hide(element) {
    element.style.display = 'none';
}

/**
 * Toggle element visibility
 * @param {Element} element - The element to toggle
 * @param {string} display - The display value when visible
 */
export function toggle(element, display = 'block') {
    if (element.style.display === 'none') {
        element.style.display = display;
    } else {
        element.style.display = 'none';
    }
}

/**
 * Set multiple attributes on an element
 * @param {Element} element - The element to modify
 * @param {Object} attrs - Attributes to set
 */
export function setAttributes(element, attrs) {
    for (const [key, value] of Object.entries(attrs)) {
        element.setAttribute(key, value);
    }
}

/**
 * Get element position relative to the viewport
 * @param {Element} element - The element
 * @returns {{top: number, left: number, bottom: number, right: number}} Position object
 */
export function getPosition(element) {
    const rect = element.getBoundingClientRect();
    return {
        top: rect.top,
        left: rect.left,
        bottom: rect.bottom,
        right: rect.right,
        width: rect.width,
        height: rect.height
    };
}

/**
 * Scroll element into view with options
 * @param {Element} element - The element to scroll to
 * @param {Object} options - Scroll options
 */
export function scrollIntoView(element, options = {}) {
    const defaultOptions = {
        behavior: 'smooth',
        block: 'center',
        inline: 'nearest'
    };
    element.scrollIntoView({ ...defaultOptions, ...options });
}

/**
 * Add event listener with automatic cleanup
 * @param {Element} element - The element
 * @param {string} event - The event name
 * @param {Function} handler - The event handler
 * @returns {Function} Cleanup function
 */
export function on(element, event, handler) {
    element.addEventListener(event, handler);
    return () => element.removeEventListener(event, handler);
}

/**
 * Add delegated event listener
 * @param {Element} parent - The parent element
 * @param {string} selector - Child selector
 * @param {string} event - The event name
 * @param {Function} handler - The event handler
 * @returns {Function} Cleanup function
 */
export function delegate(parent, selector, event, handler) {
    const delegatedHandler = (e) => {
        const target = e.target.closest(selector);
        if (target && parent.contains(target)) {
            handler.call(target, e);
        }
    };
    parent.addEventListener(event, delegatedHandler);
    return () => parent.removeEventListener(event, delegatedHandler);
}

/**
 * Wait for an element to appear in the DOM
 * @param {string} selector - CSS selector
 * @param {number} timeout - Maximum wait time in milliseconds
 * @returns {Promise<Element>} Promise that resolves with the element
 */
export function waitForElement(selector, timeout = 5000) {
    return new Promise((resolve, reject) => {
        const element = document.querySelector(selector);
        if (element) {
            return resolve(element);
        }
        
        const observer = new MutationObserver((mutations, obs) => {
            const element = document.querySelector(selector);
            if (element) {
                obs.disconnect();
                resolve(element);
            }
        });
        
        observer.observe(document.body, {
            childList: true,
            subtree: true
        });
        
        setTimeout(() => {
            observer.disconnect();
            reject(new Error(`Element ${selector} not found within ${timeout}ms`));
        }, timeout);
    });
}

/**
 * Animate an element using the Web Animations API
 * @param {Element} element - The element to animate
 * @param {Array} keyframes - Animation keyframes
 * @param {Object} options - Animation options
 * @returns {Animation} The animation object
 */
export function animate(element, keyframes, options = {}) {
    const defaultOptions = {
        duration: 300,
        easing: 'ease-in-out',
        fill: 'forwards'
    };
    return element.animate(keyframes, { ...defaultOptions, ...options });
}

/**
 * Insert element after another element
 * @param {Element} newElement - The element to insert
 * @param {Element} referenceElement - The reference element
 */
export function insertAfter(newElement, referenceElement) {
    referenceElement.parentNode.insertBefore(newElement, referenceElement.nextSibling);
}

/**
 * Wrap an element with another element
 * @param {Element} element - The element to wrap
 * @param {Element} wrapper - The wrapper element
 */
export function wrap(element, wrapper) {
    element.parentNode.insertBefore(wrapper, element);
    wrapper.appendChild(element);
}

/**
 * Unwrap an element (remove its parent)
 * @param {Element} element - The element to unwrap
 */
export function unwrap(element) {
    const parent = element.parentNode;
    if (parent && parent !== document.body) {
        while (element.firstChild) {
            parent.insertBefore(element.firstChild, element);
        }
        parent.removeChild(element);
    }
}

// Export for non-module environments
if (typeof window !== 'undefined') {
    window.DOMUtils = {
        $,
        $$,
        createElement,
        clearElement,
        setChildren,
        toggleClass,
        show,
        hide,
        toggle,
        setAttributes,
        getPosition,
        scrollIntoView,
        on,
        delegate,
        waitForElement,
        animate,
        insertAfter,
        wrap,
        unwrap
    };
}