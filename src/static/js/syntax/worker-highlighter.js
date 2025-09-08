// Worker-based syntax highlighter
// Manages Web Worker for offloading highlighting to background thread

export class WorkerHighlighter {
    constructor() {
        this.worker = null;
        this.pending = new Map();
        this.nextId = 1;
        this.ready = false;
        this.queue = [];
        this.fallbackHighlighter = null;
        
        this.init();
    }

    async init() {
        try {
            // Create worker
            this.worker = new Worker('/js/workers/highlight-worker.js');
            
            // Set up message handler
            this.worker.addEventListener('message', (event) => {
                const data = event.data;
                
                if (data.ready) {
                    this.ready = true;
                    console.log('Highlight worker ready');
                    this.processQueue();
                    return;
                }
                
                // Handle highlighting result
                const { id, highlighted, success, error } = data;
                const pending = this.pending.get(id);
                
                if (pending) {
                    if (success) {
                        pending.resolve(highlighted);
                    } else {
                        pending.reject(new Error(error));
                    }
                    this.pending.delete(id);
                }
            });
            
            // Set up error handler
            this.worker.addEventListener('error', (error) => {
                console.error('Worker error:', error);
                this.handleWorkerError(error);
            });
            
        } catch (error) {
            console.warn('Failed to create Web Worker, falling back to main thread:', error);
            this.initFallback();
        }
    }

    initFallback() {
        // Lazy load fallback highlighter
        import('./highlighter.js').then(module => {
            this.fallbackHighlighter = new module.SyntaxHighlighter();
            this.ready = true;
            this.processQueue();
        });
    }

    async highlight(text, language = '') {
        // If using fallback, use it directly
        if (this.fallbackHighlighter) {
            return this.fallbackHighlighter.highlight(text, language);
        }
        
        // If worker not ready, queue the request
        if (!this.ready) {
            return new Promise((resolve, reject) => {
                this.queue.push({ text, language, resolve, reject });
            });
        }
        
        // Create request
        const id = this.nextId++;
        
        return new Promise((resolve, reject) => {
            // Store pending promise
            this.pending.set(id, { resolve, reject });
            
            // Set timeout for worker response
            const timeout = setTimeout(() => {
                if (this.pending.has(id)) {
                    this.pending.delete(id);
                    reject(new Error('Highlight timeout'));
                }
            }, 5000);
            
            // Wrap resolve to clear timeout
            const originalResolve = resolve;
            resolve = (result) => {
                clearTimeout(timeout);
                originalResolve(result);
            };
            
            // Send to worker
            try {
                this.worker.postMessage({ id, text, language });
            } catch (error) {
                clearTimeout(timeout);
                this.pending.delete(id);
                reject(error);
            }
        });
    }

    processQueue() {
        while (this.queue.length > 0 && this.ready) {
            const { text, language, resolve, reject } = this.queue.shift();
            this.highlight(text, language).then(resolve).catch(reject);
        }
    }

    handleWorkerError(error) {
        console.error('Worker crashed, switching to fallback:', error);
        
        // Reject all pending
        for (const [id, pending] of this.pending) {
            pending.reject(new Error('Worker crashed'));
        }
        this.pending.clear();
        
        // Terminate worker
        if (this.worker) {
            this.worker.terminate();
            this.worker = null;
        }
        
        // Switch to fallback
        this.initFallback();
    }

    terminate() {
        if (this.worker) {
            this.worker.terminate();
            this.worker = null;
        }
        this.pending.clear();
        this.queue = [];
    }
}

// Debounced highlighting for better performance
export class DebouncedHighlighter {
    constructor(highlighter, delay = 100) {
        this.highlighter = highlighter;
        this.delay = delay;
        this.timeouts = new Map();
    }

    async highlight(element, text, language = '') {
        const key = element.id || element;
        
        // Clear existing timeout
        if (this.timeouts.has(key)) {
            clearTimeout(this.timeouts.get(key));
        }
        
        return new Promise((resolve) => {
            const timeout = setTimeout(async () => {
                try {
                    const highlighted = await this.highlighter.highlight(text, language);
                    if (element.innerHTML !== undefined) {
                        element.innerHTML = highlighted;
                    }
                    resolve(highlighted);
                } catch (error) {
                    console.error('Highlighting error:', error);
                    // Fallback to plain text
                    if (element.innerHTML !== undefined) {
                        element.textContent = text;
                    }
                    resolve(text);
                } finally {
                    this.timeouts.delete(key);
                }
            }, this.delay);
            
            this.timeouts.set(key, timeout);
        });
    }

    clear(key) {
        if (this.timeouts.has(key)) {
            clearTimeout(this.timeouts.get(key));
            this.timeouts.delete(key);
        }
    }

    clearAll() {
        for (const timeout of this.timeouts.values()) {
            clearTimeout(timeout);
        }
        this.timeouts.clear();
    }
}