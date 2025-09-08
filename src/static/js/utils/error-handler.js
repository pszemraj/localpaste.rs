// Error handling utilities

/**
 * Error boundary for catching and handling errors gracefully
 */
export class ErrorBoundary {
    constructor(container, fallback) {
        this.container = container;
        this.fallback = fallback || '<div class="error">An error occurred. Please refresh the page.</div>';
    }

    /**
     * Wrap a function with error handling
     * @param {Function} fn - The function to wrap
     * @returns {Function} The wrapped function
     */
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

/**
 * Global error handler
 */
export class ErrorHandler {
    constructor() {
        this.handlers = new Map();
        this.defaultHandler = (error) => {
            console.error('Unhandled error:', error);
        };
        
        // Set up global error handlers
        this.setupGlobalHandlers();
    }
    
    /**
     * Set up global error handlers
     */
    setupGlobalHandlers() {
        // Handle unhandled promise rejections
        window.addEventListener('unhandledrejection', (event) => {
            console.error('Unhandled promise rejection:', event.reason);
            this.handle(event.reason, 'promise-rejection');
            event.preventDefault();
        });
        
        // Handle global errors
        window.addEventListener('error', (event) => {
            console.error('Global error:', event.error);
            this.handle(event.error, 'global-error');
        });
    }
    
    /**
     * Register an error handler for a specific type
     * @param {string} type - The error type
     * @param {Function} handler - The error handler
     */
    register(type, handler) {
        this.handlers.set(type, handler);
    }
    
    /**
     * Handle an error
     * @param {Error} error - The error to handle
     * @param {string} type - The error type
     */
    handle(error, type = 'default') {
        const handler = this.handlers.get(type) || this.defaultHandler;
        handler(error);
    }
    
    /**
     * Create a wrapped function that handles errors
     * @param {Function} fn - The function to wrap
     * @param {string} type - The error type for handling
     * @returns {Function} The wrapped function
     */
    createSafeFunction(fn, type = 'default') {
        return async (...args) => {
            try {
                return await fn(...args);
            } catch (error) {
                this.handle(error, type);
                return null;
            }
        };
    }
}

/**
 * Retry an operation with exponential backoff
 * @param {Function} operation - The async operation to retry
 * @param {Object} options - Retry options
 * @returns {Promise} The result of the operation
 */
export async function retryOperation(operation, options = {}) {
    const {
        maxRetries = 3,
        delay = 1000,
        backoff = 2,
        onRetry = () => {}
    } = options;
    
    let lastError;
    
    for (let i = 0; i < maxRetries; i++) {
        try {
            return await operation();
        } catch (error) {
            lastError = error;
            
            if (i < maxRetries - 1) {
                const waitTime = delay * Math.pow(backoff, i);
                onRetry(i + 1, waitTime, error);
                await new Promise(resolve => setTimeout(resolve, waitTime));
            }
        }
    }
    
    throw lastError;
}

/**
 * Create a timeout wrapper for async functions
 * @param {Function} fn - The async function to wrap
 * @param {number} timeout - Timeout in milliseconds
 * @returns {Function} The wrapped function
 */
export function withTimeout(fn, timeout) {
    return async (...args) => {
        const timeoutPromise = new Promise((_, reject) => {
            setTimeout(() => reject(new Error(`Operation timed out after ${timeout}ms`)), timeout);
        });
        
        return Promise.race([
            fn(...args),
            timeoutPromise
        ]);
    };
}

/**
 * Safe JSON parse with error handling
 * @param {string} text - The JSON text to parse
 * @param {*} fallback - Fallback value on error
 * @returns {*} The parsed JSON or fallback
 */
export function safeJsonParse(text, fallback = null) {
    try {
        return JSON.parse(text);
    } catch (error) {
        console.error('JSON parse error:', error);
        return fallback;
    }
}

/**
 * Safe function call that catches and logs errors
 * @param {Function} fn - The function to call
 * @param {...*} args - Arguments to pass to the function
 * @returns {*} The function result or undefined on error
 */
export function safeCall(fn, ...args) {
    try {
        return fn(...args);
    } catch (error) {
        console.error('Safe call error:', error);
        return undefined;
    }
}

// Export for non-module environments
if (typeof window !== 'undefined') {
    window.ErrorBoundary = ErrorBoundary;
    window.ErrorHandler = ErrorHandler;
    window.retryOperation = retryOperation;
    window.withTimeout = withTimeout;
    window.safeJsonParse = safeJsonParse;
    window.safeCall = safeCall;
}