/**
 * Main App Component
 * Orchestrates all components and manages the application lifecycle
 */

import { Editor } from './Editor.js';
import { Sidebar } from './Sidebar.js';

export class App {
    constructor() {
        // Wait for dependencies
        this.store = null;
        this.api = null;
        this.editor = null;
        this.sidebar = null;
        
        this.init();
    }

    async init() {
        try {
            // Initialize API client
            if (window.ApiClient) {
                this.api = new window.ApiClient();
            } else {
                console.error('API client not available');
                return;
            }

            // Initialize store
            if (window.Store) {
                this.store = new window.Store();
            } else {
                console.error('Store not available');
                return;
            }

            // Load initial data
            await this.loadInitialData();

            // Mount components
            this.mountComponents();

            // Set up keyboard shortcuts
            this.setupKeyboardShortcuts();

            // Restore last opened paste
            this.restoreLastPaste();

            console.log('App initialized successfully');
        } catch (error) {
            console.error('App initialization failed:', error);
            this.showError('Failed to initialize application');
        }
    }

    async loadInitialData() {
        try {
            // Load folders and pastes in parallel
            const [folders, pastes] = await Promise.all([
                this.api.listFolders(),
                this.api.listPastes()
            ]);

            // Update store
            if (window.StoreActions) {
                this.store.dispatch(window.StoreActions.setFolders(folders));
                this.store.dispatch(window.StoreActions.setPastes(pastes));
            }

            console.log(`Loaded ${pastes.length} pastes and ${folders.length} folders`);
        } catch (error) {
            console.error('Failed to load initial data:', error);
            throw error;
        }
    }

    mountComponents() {
        // Mount sidebar
        const sidebarContainer = document.querySelector('.sidebar');
        if (sidebarContainer) {
            this.sidebar = new Sidebar(this.store, this.api);
            this.sidebar.mount(sidebarContainer);
        }

        // Mount editor
        const editorContainer = document.querySelector('.main-content');
        if (editorContainer) {
            this.editor = new Editor(this.store, this.api);
            this.editor.mount(editorContainer);
        }

        // Subscribe to store for app-level concerns
        this.subscribeToStore();
    }

    subscribeToStore() {
        if (!this.store) return;

        this.store.subscribe((event) => {
            const { action } = event.detail;

            // Save last opened paste to localStorage
            if (action.type === 'SET_CURRENT_PASTE') {
                const pasteId = this.store.state.currentPasteId;
                if (pasteId) {
                    localStorage.setItem('lastOpenedPaste', pasteId);
                }
            }

            // Handle errors
            if (action.type === 'ERROR') {
                this.showError(action.payload);
            }
        });
    }

    async restoreLastPaste() {
        const lastPasteId = localStorage.getItem('lastOpenedPaste');
        if (!lastPasteId) {
            // If no last paste, create or select first one
            const pastes = this.store.getPastesArray();
            if (pastes.length > 0) {
                await this.selectPaste(pastes[0].id);
            } else {
                await this.createNewPaste();
            }
            return;
        }

        // Try to load the last paste
        try {
            await this.selectPaste(lastPasteId);
        } catch (error) {
            console.warn('Could not restore last paste:', error);
            // Fall back to first paste or create new
            const pastes = this.store.getPastesArray();
            if (pastes.length > 0) {
                await this.selectPaste(pastes[0].id);
            } else {
                await this.createNewPaste();
            }
        }
    }

    async selectPaste(pasteId) {
        try {
            const paste = await this.api.getPaste(pasteId);
            
            if (window.StoreActions) {
                this.store.dispatch(window.StoreActions.setCurrentPaste(paste.id));
                this.store.dispatch(window.StoreActions.updatePaste(paste));
            }
        } catch (error) {
            console.error('Failed to select paste:', error);
            throw error;
        }
    }

    async createNewPaste() {
        try {
            const paste = await this.api.createPaste({
                name: 'Untitled',
                content: '',
                language: ''
            });

            if (window.StoreActions) {
                this.store.dispatch(window.StoreActions.addPaste(paste));
                this.store.dispatch(window.StoreActions.setCurrentPaste(paste.id));
            }

            return paste;
        } catch (error) {
            console.error('Failed to create new paste:', error);
            throw error;
        }
    }

    setupKeyboardShortcuts() {
        document.addEventListener('keydown', async (e) => {
            // Ctrl/Cmd + N: New paste
            if ((e.ctrlKey || e.metaKey) && e.key === 'n') {
                e.preventDefault();
                await this.createNewPaste();
            }

            // Ctrl/Cmd + K: Focus search
            if ((e.ctrlKey || e.metaKey) && e.key === 'k') {
                e.preventDefault();
                const searchInput = document.querySelector('#search');
                if (searchInput) {
                    searchInput.focus();
                    searchInput.select();
                }
            }

            // Ctrl/Cmd + S: Save (though auto-save handles this)
            if ((e.ctrlKey || e.metaKey) && e.key === 's') {
                e.preventDefault();
                if (this.editor) {
                    this.editor.save();
                }
            }

            // Ctrl/Cmd + Shift + E: Export
            if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key === 'E') {
                e.preventDefault();
                if (this.sidebar) {
                    this.sidebar.exportAll();
                }
            }
        });
    }

    showError(message) {
        console.error('App error:', message);
        
        // Show in UI if status element exists
        const statusEl = document.querySelector('#status');
        if (statusEl) {
            statusEl.textContent = `Error: ${message}`;
            statusEl.classList.add('error');
            
            setTimeout(() => {
                statusEl.classList.remove('error');
                statusEl.textContent = 'Ready';
            }, 5000);
        }
    }

    destroy() {
        // Unmount components
        if (this.editor) {
            this.editor.unmount();
            this.editor = null;
        }

        if (this.sidebar) {
            this.sidebar.unmount();
            this.sidebar = null;
        }

        // Clear references
        this.store = null;
        this.api = null;
    }
}