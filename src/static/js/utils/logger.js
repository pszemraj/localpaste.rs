/**
 * Console reporter for sending client-side errors to the server
 * This helps debug JavaScript issues by showing them in the terminal
 */
export class ConsoleReporter {
    constructor(endpoint = '/api/log') {
        this.endpoint = endpoint;
        this.isDev = window.location.hostname === 'localhost' || 
                     window.location.hostname === '127.0.0.1';
        
        // Store original console methods
        this.originalConsole = {
            error: console.error.bind(console),
            warn: console.warn.bind(console),
            log: console.log.bind(console),
            info: console.info.bind(console)
        };
        
        this.setupErrorHandlers();
        this.interceptConsole();
        
        if (this.isDev) {
            console.info('📡 Console reporter initialized - errors will be sent to server');
        }
    }
    
    setupErrorHandlers() {
        // Catch all uncaught errors
        window.addEventListener('error', (e) => {
            this.report('error', e.message || 'Unknown error', {
                stack: e.error?.stack || '',
                source: `${e.filename}:${e.lineno}:${e.colno}`
            });
        });
        
        // Catch promise rejections
        window.addEventListener('unhandledrejection', (e) => {
            this.report('error', `Unhandled Promise: ${e.reason}`, {
                stack: e.reason?.stack || String(e.reason)
            });
        });
    }
    
    interceptConsole() {
        // Always intercept errors
        console.error = (...args) => {
            this.originalConsole.error(...args);
            const message = this.formatArgs(args);
            const stack = new Error().stack;
            this.report('error', message, { stack });
        };
        
        // Always intercept warnings
        console.warn = (...args) => {
            this.originalConsole.warn(...args);
            this.report('warn', this.formatArgs(args));
        };
        
        // Only log info/log in dev mode
        if (this.isDev) {
            console.log = (...args) => {
                this.originalConsole.log(...args);
                // Only send important logs to server (not every single one)
                const message = this.formatArgs(args);
                if (this.isImportantLog(message)) {
                    this.report('info', message);
                }
            };
            
            console.info = (...args) => {
                this.originalConsole.info(...args);
                const message = this.formatArgs(args);
                if (this.isImportantLog(message)) {
                    this.report('info', message);
                }
            };
        }
    }
    
    formatArgs(args) {
        return args.map(arg => {
            if (typeof arg === 'object') {
                try {
                    return JSON.stringify(arg, null, 2);
                } catch {
                    return String(arg);
                }
            }
            return String(arg);
        }).join(' ');
    }
    
    isImportantLog(message) {
        // Filter out noise, only send important logs
        const important = [
            '🔴', '🟡', '🔵', '📡', '📦', '📊',  // Our emoji markers
            'Error', 'Failed', 'Exception',
            'API', 'Loaded', 'Initialized',
            '→', '←',  // Function traces
            'Action:', 'State'  // Redux-style logs
        ];
        
        return important.some(keyword => message.includes(keyword));
    }
    
    async report(level, message, extra = {}) {
        // Don't report in production unless it's an error
        if (!this.isDev && level !== 'error') {
            return;
        }
        
        try {
            await fetch(this.endpoint, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    level,
                    message,
                    ...extra,
                    timestamp: new Date().toISOString(),
                    url: window.location.href,
                    userAgent: navigator.userAgent
                })
            });
        } catch (e) {
            // Silently fail - don't want reporting errors to cause errors
            this.originalConsole.error('Failed to report to server:', e);
        }
    }
}

/**
 * Development helper for tracing function calls
 */
export function trace(name, fn) {
    return (...args) => {
        console.log(`→ ${name}`, args);
        try {
            const result = fn(...args);
            
            // Handle promises
            if (result && typeof result.then === 'function') {
                return result
                    .then(value => {
                        console.log(`← ${name} (resolved)`, value);
                        return value;
                    })
                    .catch(error => {
                        console.error(`✗ ${name} (rejected)`, error);
                        throw error;
                    });
            }
            
            console.log(`← ${name}`, result);
            return result;
        } catch (e) {
            console.error(`✗ ${name}`, e);
            throw e;
        }
    };
}

/**
 * Development helper for debugging
 */
export function setupDebugMode() {
    if (window.location.hostname === 'localhost' || window.location.hostname === '127.0.0.1') {
        window.DEBUG = true;
        console.info('🐛 Debug mode enabled');
        
        // Add global helpers
        window.trace = trace;
        
        // Log when DOM is ready
        if (document.readyState === 'loading') {
            document.addEventListener('DOMContentLoaded', () => {
                console.info('📄 DOM ready');
            });
        } else {
            console.info('📄 DOM already loaded');
        }
        
        // Log when all resources are loaded
        window.addEventListener('load', () => {
            console.info('✅ All resources loaded');
        });
    }
}