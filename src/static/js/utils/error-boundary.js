/**
 * Error Boundary for catching and handling errors gracefully
 */
export class ErrorBoundary {
    constructor(container, fallback = '<div class="error">Something went wrong. Please refresh.</div>') {
        this.container = container;
        this.fallback = fallback;
        this.errorHandlers = [];
    }

    /**
     * Wrap a function with error handling
     */
    wrap(fn) {
        return async (...args) => {
            try {
                return await fn(...args);
            } catch (error) {
                console.error('Error boundary caught:', error);
                this.handleError(error);
                throw error;
            }
        };
    }

    /**
     * Wrap a sync function
     */
    wrapSync(fn) {
        return (...args) => {
            try {
                return fn(...args);
            } catch (error) {
                console.error('Error boundary caught:', error);
                this.handleError(error);
                throw error;
            }
        };
    }

    /**
     * Handle an error
     */
    handleError(error) {
        // Log to console
        console.error('Error:', error);
        
        // Send to server if available
        if (window.log && window.log.error) {
            window.log.error('UI Error', {
                message: error.message,
                stack: error.stack
            });
        }
        
        // Call registered handlers
        this.errorHandlers.forEach(handler => {
            try {
                handler(error);
            } catch (e) {
                console.error('Error in error handler:', e);
            }
        });
        
        // Show fallback UI if container provided
        if (this.container && this.fallback) {
            if (typeof this.container === 'string') {
                const el = document.querySelector(this.container);
                if (el) el.innerHTML = this.fallback;
            } else if (this.container.innerHTML !== undefined) {
                this.container.innerHTML = this.fallback;
            }
        }
    }

    /**
     * Register an error handler
     */
    onError(handler) {
        this.errorHandlers.push(handler);
    }

    /**
     * Create a global error boundary
     */
    static setupGlobal() {
        const boundary = new ErrorBoundary(null, null);
        
        // Catch unhandled errors
        window.addEventListener('error', (event) => {
            boundary.handleError(new Error(event.message));
        });
        
        // Catch unhandled promise rejections
        window.addEventListener('unhandledrejection', (event) => {
            boundary.handleError(new Error(event.reason));
        });
        
        return boundary;
    }
}

// Export for global use
window.ErrorBoundary = ErrorBoundary;