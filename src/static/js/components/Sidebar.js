/**
 * Sidebar Component
 * Manages the paste list, folders, search, and actions
 */

export class Sidebar {
    constructor(store, api) {
        this.store = store;
        this.api = api;
        this.element = null;
        this.searchDebounced = null;
        this.cleanup = null;
        
        // Use debounce for search
        if (window.CommonUtils && window.CommonUtils.debounce) {
            this.searchDebounced = window.CommonUtils.debounce(this.handleSearch.bind(this), 300);
        } else {
            let timeout;
            this.searchDebounced = (query) => {
                clearTimeout(timeout);
                timeout = setTimeout(() => this.handleSearch(query), 300);
            };
        }
    }

    mount(container) {
        this.element = container;
        this.render();
        this.attachListeners();
        this.subscribeToStore();
    }

    render() {
        this.element.innerHTML = `
            <div class="sidebar-content">
                <div class="logo"><h1>LocalPaste.rs</h1></div>
                
                <div class="quick-actions">
                    <button id="new-paste" class="btn-primary" title="New Paste (Ctrl+N)">
                        + New
                    </button>
                    <button id="new-folder" class="btn-secondary" title="New Folder">
                        + Folder
                    </button>
                </div>
                
                <div class="search-container">
                    <input type="text" id="search" placeholder="Search... (Ctrl+K)" />
                </div>
                
                <div class="sort-container">
                    <select id="sort-order" title="Sort pastes">
                        <option value="date-desc">⬇ Newest first</option>
                        <option value="date-asc">⬆ Oldest first</option>
                        <option value="name-asc">⬆ Name (A-Z)</option>
                        <option value="name-desc">⬇ Name (Z-A)</option>
                    </select>
                </div>
                
                <div class="pastes-container">
                    <ul id="tree-view" class="tree-view">
                        <!-- Tree will be rendered here -->
                    </ul>
                </div>
                
                <div class="sidebar-footer">
                    <button id="export-all" class="btn-link" title="Export All">
                        Export
                    </button>
                    <button id="import-data" class="btn-link" title="Import">
                        Import
                    </button>
                </div>
            </div>
        `;

        // Render initial tree
        this.renderTree();
    }

    attachListeners() {
        // New paste button
        const newPasteBtn = this.element.querySelector('#new-paste');
        if (newPasteBtn) {
            newPasteBtn.addEventListener('click', () => this.createNewPaste());
        }

        // New folder button
        const newFolderBtn = this.element.querySelector('#new-folder');
        if (newFolderBtn) {
            newFolderBtn.addEventListener('click', () => this.createNewFolder());
        }

        // Search input
        const searchInput = this.element.querySelector('#search');
        if (searchInput) {
            searchInput.addEventListener('input', (e) => this.searchDebounced(e.target.value));
        }

        // Sort order
        const sortSelect = this.element.querySelector('#sort-order');
        if (sortSelect) {
            sortSelect.addEventListener('change', (e) => this.handleSortChange(e.target.value));
        }

        // Export/Import buttons
        const exportBtn = this.element.querySelector('#export-all');
        if (exportBtn) {
            exportBtn.addEventListener('click', () => this.exportAll());
        }

        const importBtn = this.element.querySelector('#import-data');
        if (importBtn) {
            importBtn.addEventListener('click', () => this.importData());
        }

        // Cleanup
        this.cleanup = () => {
            if (newPasteBtn) newPasteBtn.removeEventListener('click', this.createNewPaste);
            if (newFolderBtn) newFolderBtn.removeEventListener('click', this.createNewFolder);
            if (searchInput) searchInput.removeEventListener('input', this.searchDebounced);
            if (sortSelect) sortSelect.removeEventListener('change', this.handleSortChange);
            if (exportBtn) exportBtn.removeEventListener('click', this.exportAll);
            if (importBtn) importBtn.removeEventListener('click', this.importData);
        };
    }

    subscribeToStore() {
        if (!this.store) return;

        this.storeUnsubscribe = this.store.subscribe((event) => {
            const { action } = event.detail;
            
            // Re-render tree on relevant changes
            const treeActions = [
                'SET_PASTES', 'ADD_PASTE', 'UPDATE_PASTE', 'DELETE_PASTE',
                'SET_FOLDERS', 'ADD_FOLDER', 'DELETE_FOLDER',
                'TOGGLE_FOLDER', 'SET_SORT_ORDER'
            ];
            
            if (treeActions.includes(action.type)) {
                this.renderTree();
            }
        });
    }

    renderTree() {
        const treeView = this.element.querySelector('#tree-view');
        if (!treeView) return;

        const pastes = this.store ? this.store.getPastesArray() : [];
        const folders = this.store ? this.store.getFoldersArray() : [];
        const expandedFolders = this.store ? 
            this.store.state.ui.expandedFolders : new Set(['unfiled']);
        const sortOrder = this.store ? 
            this.store.state.ui.sortOrder : 'date-desc';

        // Sort pastes
        const sortedPastes = this.sortPastes(pastes, sortOrder);

        // Group pastes by folder
        const pastesByFolder = {};
        const unfiledPastes = [];

        sortedPastes.forEach(paste => {
            if (paste.folder_id) {
                if (!pastesByFolder[paste.folder_id]) {
                    pastesByFolder[paste.folder_id] = [];
                }
                pastesByFolder[paste.folder_id].push(paste);
            } else {
                unfiledPastes.push(paste);
            }
        });

        // Build tree HTML
        let html = '';

        // Render folders
        folders.forEach(folder => {
            const isExpanded = expandedFolders.has(folder.id);
            const folderPastes = pastesByFolder[folder.id] || [];
            
            html += `
                <li class="tree-folder ${isExpanded ? 'expanded' : ''}">
                    <div class="folder-header" data-folder-id="${folder.id}">
                        <span class="folder-toggle">${isExpanded ? '▼' : '▶'}</span>
                        <span class="folder-name">${this.escapeHtml(folder.name)}</span>
                        <span class="folder-count">(${folderPastes.length})</span>
                        <div class="folder-actions">
                            <button class="folder-rename" title="Rename">✏️</button>
                            <button class="folder-delete" title="Delete">🗑️</button>
                        </div>
                    </div>
                    <ul class="tree-contents" style="${isExpanded ? '' : 'display: none'}">
                        ${folderPastes.length > 0 ? 
                            folderPastes.map(paste => this.renderPasteItem(paste)).join('') :
                            '<li class="empty-state">Empty folder</li>'
                        }
                    </ul>
                </li>
            `;
        });

        // Render unfiled pastes
        if (unfiledPastes.length > 0) {
            const isExpanded = expandedFolders.has('unfiled');
            html += `
                <li class="tree-folder ${isExpanded ? 'expanded' : ''}">
                    <div class="folder-header" data-folder-id="unfiled">
                        <span class="folder-toggle">${isExpanded ? '▼' : '▶'}</span>
                        <span class="folder-name">Unfiled</span>
                        <span class="folder-count">(${unfiledPastes.length})</span>
                    </div>
                    <ul class="tree-contents" style="${isExpanded ? '' : 'display: none'}">
                        ${unfiledPastes.map(paste => this.renderPasteItem(paste)).join('')}
                    </ul>
                </li>
            `;
        }

        treeView.innerHTML = html;

        // Attach tree event listeners
        this.attachTreeListeners();
    }

    renderPasteItem(paste) {
        const currentPasteId = this.store ? this.store.state.currentPasteId : null;
        const isActive = paste.id === currentPasteId;
        
        return `
            <li class="tree-item ${isActive ? 'active' : ''}" 
                data-paste-id="${paste.id}" 
                draggable="true">
                <span class="paste-name">${this.escapeHtml(paste.name || 'Untitled')}</span>
                <button class="paste-delete" title="Delete">×</button>
            </li>
        `;
    }

    attachTreeListeners() {
        const treeView = this.element.querySelector('#tree-view');
        if (!treeView) return;

        // Folder toggles
        treeView.querySelectorAll('.folder-header').forEach(header => {
            header.addEventListener('click', (e) => {
                if (!e.target.closest('.folder-actions')) {
                    const folderId = header.dataset.folderId;
                    this.toggleFolder(folderId);
                }
            });
        });

        // Folder actions
        treeView.querySelectorAll('.folder-rename').forEach(btn => {
            btn.addEventListener('click', (e) => {
                e.stopPropagation();
                const folderId = btn.closest('.folder-header').dataset.folderId;
                if (folderId !== 'unfiled') {
                    this.renameFolder(folderId);
                }
            });
        });

        treeView.querySelectorAll('.folder-delete').forEach(btn => {
            btn.addEventListener('click', (e) => {
                e.stopPropagation();
                const folderId = btn.closest('.folder-header').dataset.folderId;
                if (folderId !== 'unfiled') {
                    this.deleteFolder(folderId);
                }
            });
        });

        // Paste items
        treeView.querySelectorAll('.tree-item').forEach(item => {
            item.addEventListener('click', (e) => {
                if (!e.target.closest('.paste-delete')) {
                    const pasteId = item.dataset.pasteId;
                    this.selectPaste(pasteId);
                }
            });

            // Drag and drop
            item.addEventListener('dragstart', (e) => {
                e.dataTransfer.effectAllowed = 'move';
                e.dataTransfer.setData('text/plain', item.dataset.pasteId);
                item.classList.add('dragging');
            });

            item.addEventListener('dragend', () => {
                item.classList.remove('dragging');
            });
        });

        // Paste delete buttons
        treeView.querySelectorAll('.paste-delete').forEach(btn => {
            btn.addEventListener('click', (e) => {
                e.stopPropagation();
                const pasteId = btn.closest('.tree-item').dataset.pasteId;
                this.deletePaste(pasteId);
            });
        });

        // Drop zones for folders
        treeView.querySelectorAll('.folder-header').forEach(header => {
            header.addEventListener('dragover', (e) => {
                e.preventDefault();
                header.classList.add('drag-over');
            });

            header.addEventListener('dragleave', () => {
                header.classList.remove('drag-over');
            });

            header.addEventListener('drop', async (e) => {
                e.preventDefault();
                header.classList.remove('drag-over');
                const pasteId = e.dataTransfer.getData('text/plain');
                const folderId = header.dataset.folderId;
                if (pasteId && folderId !== 'unfiled') {
                    await this.movePasteToFolder(pasteId, folderId);
                }
            });
        });
    }

    sortPastes(pastes, sortOrder) {
        const sorted = [...pastes];
        
        switch (sortOrder) {
            case 'date-desc':
                sorted.sort((a, b) => new Date(b.updated_at) - new Date(a.updated_at));
                break;
            case 'date-asc':
                sorted.sort((a, b) => new Date(a.updated_at) - new Date(b.updated_at));
                break;
            case 'name-asc':
                sorted.sort((a, b) => (a.name || '').localeCompare(b.name || ''));
                break;
            case 'name-desc':
                sorted.sort((a, b) => (b.name || '').localeCompare(a.name || ''));
                break;
        }
        
        return sorted;
    }

    async createNewPaste() {
        if (!this.api) return;

        try {
            const paste = await this.api.createPaste({
                name: 'Untitled',
                content: '',
                language: ''
            });

            if (this.store && window.StoreActions) {
                this.store.dispatch(window.StoreActions.addPaste(paste));
                this.store.dispatch(window.StoreActions.setCurrentPaste(paste.id));
            }
        } catch (error) {
            console.error('Failed to create paste:', error);
        }
    }

    async createNewFolder() {
        const name = prompt('Folder name:');
        if (!name || !this.api) return;

        try {
            const folder = await this.api.createFolder({ name });
            
            if (this.store && window.StoreActions) {
                this.store.dispatch(window.StoreActions.addFolder(folder));
            }
        } catch (error) {
            console.error('Failed to create folder:', error);
        }
    }

    async selectPaste(pasteId) {
        if (!this.api) return;

        try {
            const paste = await this.api.getPaste(pasteId);
            
            if (this.store && window.StoreActions) {
                this.store.dispatch(window.StoreActions.setCurrentPaste(paste.id));
                this.store.dispatch(window.StoreActions.updatePaste(paste));
            }
        } catch (error) {
            console.error('Failed to load paste:', error);
        }
    }

    async deletePaste(pasteId) {
        if (!confirm('Delete this paste?') || !this.api) return;

        try {
            await this.api.deletePaste(pasteId);
            
            if (this.store && window.StoreActions) {
                this.store.dispatch(window.StoreActions.deletePaste(pasteId));
            }
        } catch (error) {
            console.error('Failed to delete paste:', error);
        }
    }

    async movePasteToFolder(pasteId, folderId) {
        if (!this.api) return;

        try {
            await this.api.updatePaste(pasteId, { folder_id: folderId });
            
            // Reload data
            const pastes = await this.api.listPastes();
            if (this.store && window.StoreActions) {
                this.store.dispatch(window.StoreActions.setPastes(pastes));
            }
        } catch (error) {
            console.error('Failed to move paste:', error);
        }
    }

    toggleFolder(folderId) {
        if (this.store && window.StoreActions) {
            this.store.dispatch(window.StoreActions.toggleFolder(folderId));
        }
    }

    async renameFolder(folderId) {
        const folder = this.store ? 
            this.store.getFoldersArray().find(f => f.id === folderId) : null;
        if (!folder) return;

        const newName = prompt('Rename folder:', folder.name);
        if (!newName || !this.api) return;

        try {
            await this.api.updateFolder(folderId, { name: newName });
            
            const folders = await this.api.listFolders();
            if (this.store && window.StoreActions) {
                this.store.dispatch(window.StoreActions.setFolders(folders));
            }
        } catch (error) {
            console.error('Failed to rename folder:', error);
        }
    }

    async deleteFolder(folderId) {
        if (!confirm('Delete this folder? Pastes will be moved to Unfiled.') || !this.api) return;

        try {
            await this.api.deleteFolder(folderId);
            
            // Reload data
            const [folders, pastes] = await Promise.all([
                this.api.listFolders(),
                this.api.listPastes()
            ]);
            
            if (this.store && window.StoreActions) {
                this.store.dispatch(window.StoreActions.setFolders(folders));
                this.store.dispatch(window.StoreActions.setPastes(pastes));
            }
        } catch (error) {
            console.error('Failed to delete folder:', error);
        }
    }

    async handleSearch(query) {
        if (!this.api) return;

        try {
            const pastes = query ? 
                await this.api.searchPastes(query) : 
                await this.api.listPastes();
            
            if (this.store && window.StoreActions) {
                this.store.dispatch(window.StoreActions.setPastes(pastes));
                this.store.dispatch(window.StoreActions.setSearchQuery(query));
            }
        } catch (error) {
            console.error('Search failed:', error);
        }
    }

    handleSortChange(sortOrder) {
        if (this.store && window.StoreActions) {
            this.store.dispatch(window.StoreActions.setSortOrder(sortOrder));
        }
    }

    async exportAll() {
        if (!this.api) return;

        try {
            const data = await this.api.exportAll();
            const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' });
            const url = URL.createObjectURL(blob);
            const a = document.createElement('a');
            a.href = url;
            a.download = `localpaste-export-${Date.now()}.json`;
            a.click();
            URL.revokeObjectURL(url);
        } catch (error) {
            console.error('Export failed:', error);
        }
    }

    async importData() {
        const input = document.createElement('input');
        input.type = 'file';
        input.accept = '.json';
        
        input.onchange = async (e) => {
            const file = e.target.files[0];
            if (!file) return;

            try {
                const text = await file.text();
                const data = JSON.parse(text);
                
                if (!this.api) return;
                await this.api.importData(data);
                
                // Reload everything
                const [folders, pastes] = await Promise.all([
                    this.api.listFolders(),
                    this.api.listPastes()
                ]);
                
                if (this.store && window.StoreActions) {
                    this.store.dispatch(window.StoreActions.setFolders(folders));
                    this.store.dispatch(window.StoreActions.setPastes(pastes));
                }
            } catch (error) {
                console.error('Import failed:', error);
                alert('Import failed: ' + error.message);
            }
        };
        
        input.click();
    }

    escapeHtml(text) {
        if (window.CommonUtils && window.CommonUtils.escapeHtml) {
            return window.CommonUtils.escapeHtml(text);
        }
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }

    unmount() {
        if (this.cleanup) {
            this.cleanup();
            this.cleanup = null;
        }
        
        if (this.storeUnsubscribe) {
            this.storeUnsubscribe();
            this.storeUnsubscribe = null;
        }
        
        if (this.element) {
            this.element.innerHTML = '';
        }
    }
}