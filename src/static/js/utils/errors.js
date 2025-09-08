/**
 * Error boundary for catching and handling errors gracefully
 */
export class ErrorBoundary {
    constructor(container, fallback) {
        this.container = container;
        this.fallback = fallback || '<div class="error">An error occurred. Please refresh the page.</div>';
    }

    wrap(fn) {
        return async (...args) => {
            try {
                return await fn(...args);
            } catch (error) {
                console.error('Error boundary caught:', error);
                // Only update container if it's a critical error
                if (error.critical) {
                    this.container.innerHTML = this.fallback;
                }
                // Re-throw for debugging
                throw error;
            }
        };
    }
}