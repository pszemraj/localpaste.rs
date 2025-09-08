/**
 * Debounce function - delays execution until after wait milliseconds have elapsed
 * since the last time the debounced function was invoked
 * @param {Function} fn - The function to debounce
 * @param {number} ms - The number of milliseconds to delay
 * @returns {Function} The debounced function
 */
export function debounce(fn, ms) {
    let timeout;
    return function(...args) {
        clearTimeout(timeout);
        timeout = setTimeout(() => fn.apply(this, args), ms);
    };
}