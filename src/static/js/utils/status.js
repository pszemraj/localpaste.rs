// Status message management

export class StatusManager {
    constructor(elementId = 'status-message', defaultMessage = 'Ready', timeout = 3000) {
        this.element = document.getElementById(elementId);
        this.defaultMessage = defaultMessage;
        this.timeout = timeout;
        this.timeoutId = null;
    }
    
    /**
     * Set a status message
     * @param {string} message - The message to display
     * @param {string} type - Message type (info, success, warning, error)
     * @param {boolean} persistent - Whether the message should persist
     */
    setStatus(message, type = 'info', persistent = false) {
        if (!this.element) return;
        
        // Clear any existing timeout
        if (this.timeoutId) {
            clearTimeout(this.timeoutId);
            this.timeoutId = null;
        }
        
        // Set the message
        this.element.textContent = message;
        
        // Set type class if needed
        this.element.className = `status-${type}`;
        
        // Auto-clear non-persistent messages
        if (!persistent && message !== this.defaultMessage) {
            this.timeoutId = setTimeout(() => {
                this.reset();
            }, this.timeout);
        }
    }
    
    /**
     * Set an info message (default)
     * @param {string} message - The message to display
     * @param {boolean} persistent - Whether the message should persist
     */
    info(message, persistent = false) {
        this.setStatus(message, 'info', persistent);
    }
    
    /**
     * Set a success message
     * @param {string} message - The message to display
     * @param {boolean} persistent - Whether the message should persist
     */
    success(message, persistent = false) {
        this.setStatus(message, 'success', persistent);
    }
    
    /**
     * Set a warning message
     * @param {string} message - The message to display
     * @param {boolean} persistent - Whether the message should persist
     */
    warning(message, persistent = false) {
        this.setStatus(message, 'warning', persistent);
    }
    
    /**
     * Set an error message
     * @param {string} message - The message to display
     * @param {boolean} persistent - Whether the message should persist
     */
    error(message, persistent = false) {
        this.setStatus(message, 'error', persistent);
    }
    
    /**
     * Reset to default message
     */
    reset() {
        if (!this.element) return;
        this.element.textContent = this.defaultMessage;
        this.element.className = 'status-info';
    }
    
    /**
     * Show a loading message
     * @param {string} message - The loading message
     */
    loading(message = 'Loading...') {
        this.setStatus(message, 'loading', true);
    }
    
    /**
     * Clear any pending timeouts
     */
    clear() {
        if (this.timeoutId) {
            clearTimeout(this.timeoutId);
            this.timeoutId = null;
        }
    }
}

/**
 * Simple status setter for backwards compatibility
 * @param {string} message - The message to display
 * @param {number} timeout - Timeout in milliseconds
 */
export function setStatus(message, timeout = 3000) {
    const el = document.getElementById('status-message');
    if (!el) return;
    
    el.textContent = message;
    if (message !== 'Ready') {
        setTimeout(() => {
            el.textContent = 'Ready';
        }, timeout);
    }
}

// Export for non-module environments
if (typeof window !== 'undefined') {
    window.StatusManager = StatusManager;
    window.setStatus = setStatus;
}