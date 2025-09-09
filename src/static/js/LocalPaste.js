/**
 * LocalPaste Application - Modular Version
 * This coordinates all the modules together
 */

import { ApiClient } from './api/client.js';
import { Store } from './state/store.js';
import { EditorComponent } from './components/Editor.js';
import { TreeViewComponent } from './components/TreeView.js';
import { SearchBarComponent } from './components/SearchBar.js';
import { StatusBarComponent } from './components/StatusBar.js';
import { SyntaxHighlighter } from './syntax/highlighter.js';
import { debounce } from './utils/debounce.js';
import { ErrorBoundary } from './utils/error-boundary.js';

export class LocalPaste {
    constructor() {
        // Core services
        this.api = new ApiClient('/api');
        this.store = new Store();
        this.errorBoundary = new ErrorBoundary(document.body);
        
        // Components
        this.components = {
            editor: null,
            treeView: null,
            searchBar: null,
            statusBar: null
        };
        
        // Track event listeners for cleanup
        this.eventListeners = [];
        
        // Bind methods
        this.init = this.errorBoundary.wrap(this.init.bind(this));
        this.handleStoreChange = this.handleStoreChange.bind(this);
    }
    
    async init() {
        console.log('Initializing LocalPaste with modular architecture...');
        
        // Initialize store with data
        await this.loadInitialData();
        
        // Set up components
        this.initializeComponents();
        
        // Set up event listeners
        this.setupEventListeners();
        
        // Subscribe to store changes
        this.store.subscribe(this.handleStoreChange);
        
        console.log('LocalPaste initialized successfully');
    }
    
    async loadInitialData() {
        try {
            // Load folders and pastes in parallel
            const [folders, pastes] = await Promise.all([
                this.api.getFolders(),
                this.api.getPastes()
            ]);
            
            // Initialize store with data
            this.store.dispatch({
                type: 'INIT_DATA',
                payload: { folders, pastes }
            });
            
        } catch (error) {
            console.error('Failed to load initial data:', error);
            this.store.dispatch({
                type: 'ERROR',
                payload: 'Failed to load data'
            });
        }
    }
    
    initializeComponents() {
        // Initialize Editor
        this.components.editor = new EditorComponent(
            this.store,
            this.api,
            document.getElementById('editor-container')
        );
        
        // Initialize TreeView
        this.components.treeView = new TreeViewComponent(
            this.store,
            this.api,
            document.getElementById('file-tree')
        );
        
        // Initialize SearchBar
        this.components.searchBar = new SearchBarComponent(
            this.store,
            this.api,
            document.getElementById('search-container')
        );
        
        // Initialize StatusBar
        this.components.statusBar = new StatusBarComponent(
            this.store,
            document.getElementById('status-bar')
        );
        
        // Mount all components
        Object.values(this.components).forEach(component => {
            if (component && component.mount) {
                component.mount();
            }
        });
    }
    
    setupEventListeners() {
        // New paste button
        const newBtn = document.getElementById('new-paste-btn');
        if (newBtn) {
            const handler = () => this.createNewPaste();
            newBtn.addEventListener('click', handler);
            this.trackEventListener(newBtn, 'click', handler);
        }
        
        // New folder button
        const newFolderBtn = document.getElementById('new-folder-btn');
        if (newFolderBtn) {
            const handler = () => this.createNewFolder();
            newFolderBtn.addEventListener('click', handler);
            this.trackEventListener(newFolderBtn, 'click', handler);
        }
        
        // Settings button
        const settingsBtn = document.getElementById('settings-btn');
        if (settingsBtn) {
            const handler = () => this.showSettings();
            settingsBtn.addEventListener('click', handler);
            this.trackEventListener(settingsBtn, 'click', handler);
        }
        
        // Window events
        const beforeUnloadHandler = (e) => this.handleBeforeUnload(e);
        window.addEventListener('beforeunload', beforeUnloadHandler);
        this.trackEventListener(window, 'beforeunload', beforeUnloadHandler);
    }
    
    handleStoreChange(event) {
        const { action, state } = event.detail;
        
        // Update components based on state changes
        switch (action.type) {
            case 'PASTE_SELECTED':
                this.components.editor?.loadPaste(action.payload);
                break;
                
            case 'PASTE_UPDATED':
                this.components.treeView?.updatePaste(action.payload);
                break;
                
            case 'FOLDER_CREATED':
                this.components.treeView?.addFolder(action.payload);
                break;
                
            case 'ERROR':
                this.components.statusBar?.showError(action.payload);
                break;
        }
    }
    
    async createNewPaste() {
        try {
            const name = prompt('Enter paste name:');
            if (!name) return;
            
            const paste = await this.api.createPaste({
                name,
                content: '',
                language: 'plaintext',
                folder_id: this.store.getState().ui.currentFolderId || 'unfiled'
            });
            
            this.store.dispatch({
                type: 'PASTE_CREATED',
                payload: paste
            });
            
            this.store.dispatch({
                type: 'PASTE_SELECTED',
                payload: paste.id
            });
            
        } catch (error) {
            console.error('Failed to create paste:', error);
            this.store.dispatch({
                type: 'ERROR',
                payload: 'Failed to create paste'
            });
        }
    }
    
    async createNewFolder() {
        try {
            const name = prompt('Enter folder name:');
            if (!name) return;
            
            const folder = await this.api.createFolder({ name });
            
            this.store.dispatch({
                type: 'FOLDER_CREATED',
                payload: folder
            });
            
        } catch (error) {
            console.error('Failed to create folder:', error);
            this.store.dispatch({
                type: 'ERROR',
                payload: 'Failed to create folder'
            });
        }
    }
    
    showSettings() {
        // TODO: Implement settings modal
        console.log('Settings not yet implemented');
    }
    
    handleBeforeUnload(e) {
        const hasUnsavedChanges = this.store.getState().ui.hasUnsavedChanges;
        if (hasUnsavedChanges) {
            e.preventDefault();
            e.returnValue = 'You have unsaved changes. Are you sure you want to leave?';
            return e.returnValue;
        }
    }
    
    trackEventListener(element, event, handler, options) {
        this.eventListeners.push({ element, event, handler, options });
    }
    
    cleanup() {
        console.log('Cleaning up LocalPaste...');
        
        // Clean up components
        Object.values(this.components).forEach(component => {
            if (component && component.unmount) {
                component.unmount();
            }
        });
        
        // Remove all event listeners
        this.eventListeners.forEach(({ element, event, handler, options }) => {
            try {
                element.removeEventListener(event, handler, options);
            } catch (e) {
                console.warn('Failed to remove listener:', e);
            }
        });
        
        this.eventListeners = [];
        
        // Unsubscribe from store
        this.store.unsubscribe(this.handleStoreChange);
    }
}

// Export for global use if needed
window.LocalPaste = LocalPaste;