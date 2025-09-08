// State management store using EventTarget for reactivity

/**
 * Action types
 */
export const ActionTypes = {
    // Paste actions
    SET_PASTES: 'SET_PASTES',
    ADD_PASTE: 'ADD_PASTE',
    UPDATE_PASTE: 'UPDATE_PASTE',
    DELETE_PASTE: 'DELETE_PASTE',
    SET_CURRENT_PASTE: 'SET_CURRENT_PASTE',
    
    // Folder actions
    SET_FOLDERS: 'SET_FOLDERS',
    ADD_FOLDER: 'ADD_FOLDER',
    UPDATE_FOLDER: 'UPDATE_FOLDER',
    DELETE_FOLDER: 'DELETE_FOLDER',
    TOGGLE_FOLDER: 'TOGGLE_FOLDER',
    
    // UI actions
    SET_SEARCH_QUERY: 'SET_SEARCH_QUERY',
    SET_SORT_ORDER: 'SET_SORT_ORDER',
    SET_STATUS: 'SET_STATUS',
    SET_LOADING: 'SET_LOADING',
    
    // Editor actions
    SET_EDITOR_CONTENT: 'SET_EDITOR_CONTENT',
    SET_EDITOR_LANGUAGE: 'SET_EDITOR_LANGUAGE',
    SET_EDITOR_DIRTY: 'SET_EDITOR_DIRTY'
};

/**
 * Store class for managing application state
 */
export class Store extends EventTarget {
    constructor(initialState = {}) {
        super();
        
        // Initialize state with defaults
        this.state = {
            pastes: new Map(),
            folders: new Map(),
            currentPasteId: null,
            editor: {
                content: '',
                language: '',
                isDirty: false
            },
            ui: {
                expandedFolders: new Set(['unfiled']),
                searchQuery: '',
                sortOrder: 'date-desc',
                status: 'Ready',
                isLoading: false
            },
            ...initialState
        };
        
        // Bind methods
        this.dispatch = this.dispatch.bind(this);
        this.select = this.select.bind(this);
        this.subscribe = this.subscribe.bind(this);
    }
    
    /**
     * Dispatch an action to update state
     * @param {Object} action - The action to dispatch
     */
    dispatch(action) {
        const prevState = this.state;
        const newState = this.reducer(this.state, action);
        
        if (newState !== prevState) {
            this.state = newState;
            
            // Emit change event with details
            this.dispatchEvent(new CustomEvent('statechange', {
                detail: {
                    action,
                    prevState,
                    state: this.state
                }
            }));
        }
    }
    
    /**
     * Main reducer function
     * @param {Object} state - Current state
     * @param {Object} action - Action to process
     * @returns {Object} New state
     */
    reducer(state, action) {
        switch (action.type) {
            // Paste actions
            case ActionTypes.SET_PASTES:
                return {
                    ...state,
                    pastes: new Map(action.payload.map(p => [p.id, p]))
                };
                
            case ActionTypes.ADD_PASTE:
                return {
                    ...state,
                    pastes: new Map(state.pastes).set(action.payload.id, action.payload)
                };
                
            case ActionTypes.UPDATE_PASTE:
                if (!state.pastes.has(action.payload.id)) return state;
                return {
                    ...state,
                    pastes: new Map(state.pastes).set(action.payload.id, {
                        ...state.pastes.get(action.payload.id),
                        ...action.payload
                    })
                };
                
            case ActionTypes.DELETE_PASTE:
                const newPastes = new Map(state.pastes);
                newPastes.delete(action.payload);
                return {
                    ...state,
                    pastes: newPastes,
                    currentPasteId: state.currentPasteId === action.payload ? null : state.currentPasteId
                };
                
            case ActionTypes.SET_CURRENT_PASTE:
                return {
                    ...state,
                    currentPasteId: action.payload
                };
                
            // Folder actions
            case ActionTypes.SET_FOLDERS:
                return {
                    ...state,
                    folders: new Map(action.payload.map(f => [f.id, f]))
                };
                
            case ActionTypes.ADD_FOLDER:
                return {
                    ...state,
                    folders: new Map(state.folders).set(action.payload.id, action.payload)
                };
                
            case ActionTypes.UPDATE_FOLDER:
                if (!state.folders.has(action.payload.id)) return state;
                return {
                    ...state,
                    folders: new Map(state.folders).set(action.payload.id, {
                        ...state.folders.get(action.payload.id),
                        ...action.payload
                    })
                };
                
            case ActionTypes.DELETE_FOLDER:
                const newFolders = new Map(state.folders);
                newFolders.delete(action.payload);
                return {
                    ...state,
                    folders: newFolders
                };
                
            case ActionTypes.TOGGLE_FOLDER:
                const expandedFolders = new Set(state.ui.expandedFolders);
                if (expandedFolders.has(action.payload)) {
                    expandedFolders.delete(action.payload);
                } else {
                    expandedFolders.add(action.payload);
                }
                return {
                    ...state,
                    ui: {
                        ...state.ui,
                        expandedFolders
                    }
                };
                
            // UI actions
            case ActionTypes.SET_SEARCH_QUERY:
                return {
                    ...state,
                    ui: {
                        ...state.ui,
                        searchQuery: action.payload
                    }
                };
                
            case ActionTypes.SET_SORT_ORDER:
                return {
                    ...state,
                    ui: {
                        ...state.ui,
                        sortOrder: action.payload
                    }
                };
                
            case ActionTypes.SET_STATUS:
                return {
                    ...state,
                    ui: {
                        ...state.ui,
                        status: action.payload
                    }
                };
                
            case ActionTypes.SET_LOADING:
                return {
                    ...state,
                    ui: {
                        ...state.ui,
                        isLoading: action.payload
                    }
                };
                
            // Editor actions
            case ActionTypes.SET_EDITOR_CONTENT:
                return {
                    ...state,
                    editor: {
                        ...state.editor,
                        content: action.payload,
                        isDirty: true
                    }
                };
                
            case ActionTypes.SET_EDITOR_LANGUAGE:
                return {
                    ...state,
                    editor: {
                        ...state.editor,
                        language: action.payload
                    }
                };
                
            case ActionTypes.SET_EDITOR_DIRTY:
                return {
                    ...state,
                    editor: {
                        ...state.editor,
                        isDirty: action.payload
                    }
                };
                
            default:
                return state;
        }
    }
    
    /**
     * Select a value from state using a selector function
     * @param {Function} selector - Selector function
     * @returns {*} Selected value
     */
    select(selector) {
        return selector(this.state);
    }
    
    /**
     * Subscribe to state changes
     * @param {Function} listener - Listener function
     * @returns {Function} Unsubscribe function
     */
    subscribe(listener) {
        this.addEventListener('statechange', listener);
        return () => this.removeEventListener('statechange', listener);
    }
    
    /**
     * Get current paste
     * @returns {Object|null} Current paste or null
     */
    getCurrentPaste() {
        if (!this.state.currentPasteId) return null;
        return this.state.pastes.get(this.state.currentPasteId) || null;
    }
    
    /**
     * Get pastes as array
     * @returns {Array} Array of pastes
     */
    getPastesArray() {
        return Array.from(this.state.pastes.values());
    }
    
    /**
     * Get folders as array
     * @returns {Array} Array of folders
     */
    getFoldersArray() {
        return Array.from(this.state.folders.values());
    }
    
    /**
     * Get pastes for a specific folder
     * @param {string|null} folderId - Folder ID or null for unfiled
     * @returns {Array} Array of pastes in folder
     */
    getPastesByFolder(folderId) {
        return this.getPastesArray().filter(p => p.folder_id === folderId);
    }
    
    /**
     * Get sorted pastes
     * @returns {Array} Sorted array of pastes
     */
    getSortedPastes() {
        const pastes = this.getPastesArray();
        const sortOrder = this.state.ui.sortOrder;
        
        switch(sortOrder) {
            case 'date-desc':
                return pastes.sort((a, b) => new Date(b.created_at) - new Date(a.created_at));
            case 'date-asc':
                return pastes.sort((a, b) => new Date(a.created_at) - new Date(b.created_at));
            case 'name-asc':
                return pastes.sort((a, b) => (a.name || '').localeCompare(b.name || ''));
            case 'name-desc':
                return pastes.sort((a, b) => (b.name || '').localeCompare(a.name || ''));
            default:
                return pastes;
        }
    }
    
    /**
     * Search pastes
     * @param {string} query - Search query
     * @returns {Array} Filtered pastes
     */
    searchPastes(query) {
        if (!query) return this.getPastesArray();
        
        const lowerQuery = query.toLowerCase();
        return this.getPastesArray().filter(paste => {
            const name = (paste.name || '').toLowerCase();
            const content = (paste.content || '').toLowerCase();
            return name.includes(lowerQuery) || content.includes(lowerQuery);
        });
    }
}

/**
 * Create action creators for common actions
 */
export const actions = {
    // Paste actions
    setPastes: (pastes) => ({ type: ActionTypes.SET_PASTES, payload: pastes }),
    addPaste: (paste) => ({ type: ActionTypes.ADD_PASTE, payload: paste }),
    updatePaste: (paste) => ({ type: ActionTypes.UPDATE_PASTE, payload: paste }),
    deletePaste: (id) => ({ type: ActionTypes.DELETE_PASTE, payload: id }),
    setCurrentPaste: (id) => ({ type: ActionTypes.SET_CURRENT_PASTE, payload: id }),
    
    // Folder actions
    setFolders: (folders) => ({ type: ActionTypes.SET_FOLDERS, payload: folders }),
    addFolder: (folder) => ({ type: ActionTypes.ADD_FOLDER, payload: folder }),
    updateFolder: (folder) => ({ type: ActionTypes.UPDATE_FOLDER, payload: folder }),
    deleteFolder: (id) => ({ type: ActionTypes.DELETE_FOLDER, payload: id }),
    toggleFolder: (id) => ({ type: ActionTypes.TOGGLE_FOLDER, payload: id }),
    
    // UI actions
    setSearchQuery: (query) => ({ type: ActionTypes.SET_SEARCH_QUERY, payload: query }),
    setSortOrder: (order) => ({ type: ActionTypes.SET_SORT_ORDER, payload: order }),
    setStatus: (status) => ({ type: ActionTypes.SET_STATUS, payload: status }),
    setLoading: (isLoading) => ({ type: ActionTypes.SET_LOADING, payload: isLoading }),
    
    // Editor actions
    setEditorContent: (content) => ({ type: ActionTypes.SET_EDITOR_CONTENT, payload: content }),
    setEditorLanguage: (language) => ({ type: ActionTypes.SET_EDITOR_LANGUAGE, payload: language }),
    setEditorDirty: (isDirty) => ({ type: ActionTypes.SET_EDITOR_DIRTY, payload: isDirty })
};

// Export for non-module environments
if (typeof window !== 'undefined') {
    window.Store = Store;
    window.StoreActions = actions;
    window.ActionTypes = ActionTypes;
}