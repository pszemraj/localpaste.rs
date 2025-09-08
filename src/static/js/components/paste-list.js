import { VirtualScroller, PaginationController } from '../utils/virtual-scroll.js';

/**
 * PasteList component with virtual scrolling and pagination
 */
export class PasteList {
    constructor(container, options = {}) {
        this.container = container;
        this.onPasteSelect = options.onPasteSelect || (() => {});
        this.onFolderToggle = options.onFolderToggle || (() => {});
        
        this.folders = new Map();
        this.pastes = new Map(); 
        this.expandedFolders = new Set(['unfiled']);
        this.currentPasteId = null;
        this.searchQuery = '';
        
        this.virtualScroller = null;
        this.paginationController = null;
        
        this.init();
    }
    
    init() {
        this.container.innerHTML = `
            <div class="paste-list-container" style="height: 100%; display: flex; flex-direction: column;">
                <div class="paste-list-header" style="flex-shrink: 0;">
                    <input type="text" 
                           id="search-input" 
                           placeholder="Search pastes..." 
                           class="search-input"
                           style="width: 100%; padding: 8px; margin-bottom: 8px;">
                </div>
                <div class="paste-list-content" style="flex: 1; overflow: hidden;">
                    <!-- Virtual scroll container will be inserted here -->
                </div>
            </div>
        `;
        
        this.searchInput = this.container.querySelector('#search-input');
        this.contentContainer = this.container.querySelector('.paste-list-content');
        
        this.setupEventListeners();
        this.setupVirtualScroller();
    }
    
    setupEventListeners() {
        let searchTimeout;
        this.searchInput.addEventListener('input', (e) => {
            clearTimeout(searchTimeout);
            searchTimeout = setTimeout(() => {
                this.search(e.target.value);
            }, 300);
        });
    }
    
    setupVirtualScroller() {
        this.virtualScroller = new VirtualScroller({
            container: this.contentContainer,
            itemHeight: 32,
            items: [],
            overscan: 5,
            renderItem: this.renderTreeItem.bind(this)
        });
        
        this.paginationController = new PaginationController({
            pageSize: 50,
            loadPage: this.loadPage.bind(this),
            onDataLoaded: this.onDataLoaded.bind(this)
        });
    }
    
    async loadPage(page, pageSize) {
        const offset = page * pageSize;
        let url = `/api/pastes?limit=${pageSize}&offset=${offset}`;
        
        if (this.searchQuery) {
            url = `/api/search?q=${encodeURIComponent(this.searchQuery)}&limit=${pageSize}`;
        }
        
        try {
            const res = await fetch(url);
            if (!res.ok) throw new Error('Failed to load pastes');
            const pastes = await res.json();
            
            // Update internal paste map
            for (const paste of pastes) {
                this.pastes.set(paste.id, paste);
            }
            
            return pastes;
        } catch (err) {
            console.error('Error loading pastes:', err);
            return [];
        }
    }
    
    onDataLoaded(allPastes) {
        // Build tree structure
        const treeItems = this.buildTreeItems(allPastes);
        this.virtualScroller.setItems(treeItems);
    }
    
    buildTreeItems(pastes) {
        const items = [];
        const folderMap = new Map();
        
        // Group pastes by folder
        for (const paste of pastes) {
            const folderId = paste.folder_id || 'unfiled';
            if (!folderMap.has(folderId)) {
                folderMap.set(folderId, []);
            }
            folderMap.get(folderId).push(paste);
        }
        
        // Add unfiled pastes first
        if (folderMap.has('unfiled')) {
            items.push({
                type: 'folder',
                id: 'unfiled',
                name: 'Unfiled Pastes',
                count: folderMap.get('unfiled').length,
                expanded: this.expandedFolders.has('unfiled')
            });
            
            if (this.expandedFolders.has('unfiled')) {
                for (const paste of folderMap.get('unfiled')) {
                    items.push({
                        type: 'paste',
                        data: paste,
                        folderId: 'unfiled'
                    });
                }
            }
        }
        
        // Add folders and their pastes
        for (const [folderId, folder] of this.folders) {
            if (folderId === 'unfiled') continue;
            
            const folderPastes = folderMap.get(folderId) || [];
            items.push({
                type: 'folder',
                id: folderId,
                name: folder.name,
                count: folderPastes.length,
                expanded: this.expandedFolders.has(folderId)
            });
            
            if (this.expandedFolders.has(folderId)) {
                for (const paste of folderPastes) {
                    items.push({
                        type: 'paste',
                        data: paste,
                        folderId: folderId
                    });
                }
            }
        }
        
        return items;
    }
    
    renderTreeItem(item, index) {
        const element = document.createElement('div');
        
        if (item.type === 'folder') {
            element.className = 'tree-folder';
            element.innerHTML = `
                <span class="folder-toggle">${item.expanded ? '▼' : '▶'}</span>
                <span class="folder-name">${this.escapeHtml(item.name)}</span>
                <span class="folder-count">(${item.count})</span>
            `;
            element.style.cssText = `
                padding: 6px 8px;
                cursor: pointer;
                font-weight: bold;
                background: var(--folder-bg, #f5f5f5);
                border-bottom: 1px solid var(--border-color, #e0e0e0);
                display: flex;
                align-items: center;
                gap: 8px;
            `;
            
            element.addEventListener('click', () => {
                this.toggleFolder(item.id);
            });
        } else if (item.type === 'paste') {
            const paste = item.data;
            const isActive = paste.id === this.currentPasteId;
            
            element.className = `tree-paste ${isActive ? 'active' : ''}`;
            element.innerHTML = `
                <span class="paste-name">${this.escapeHtml(paste.name || 'Untitled')}</span>
                ${paste.language ? `<span class="paste-lang">${paste.language}</span>` : ''}
            `;
            element.style.cssText = `
                padding: 6px 8px 6px 24px;
                cursor: pointer;
                border-bottom: 1px solid var(--border-color, #f0f0f0);
                display: flex;
                align-items: center;
                justify-content: space-between;
                ${isActive ? 'background: var(--active-bg, #e3f2fd);' : ''}
            `;
            
            element.addEventListener('click', () => {
                this.selectPaste(paste.id);
            });
        }
        
        return element;
    }
    
    toggleFolder(folderId) {
        if (this.expandedFolders.has(folderId)) {
            this.expandedFolders.delete(folderId);
        } else {
            this.expandedFolders.add(folderId);
        }
        
        // Rebuild tree with new expanded state
        const allPastes = Array.from(this.pastes.values());
        this.onDataLoaded(allPastes);
        
        this.onFolderToggle(folderId, this.expandedFolders.has(folderId));
    }
    
    selectPaste(pasteId) {
        this.currentPasteId = pasteId;
        const paste = this.pastes.get(pasteId);
        if (paste) {
            this.onPasteSelect(paste);
        }
        
        // Refresh display to show active state
        const allPastes = Array.from(this.pastes.values());
        this.onDataLoaded(allPastes);
    }
    
    async setFolders(folders) {
        this.folders.clear();
        for (const folder of folders) {
            this.folders.set(folder.id, folder);
        }
    }
    
    async loadInitial() {
        // Load first page of pastes
        await this.paginationController.loadNextPage();
        
        // Load more pages if viewport can show more
        const containerHeight = this.contentContainer.clientHeight;
        const itemHeight = 32;
        const visibleCount = Math.ceil(containerHeight / itemHeight);
        
        while (this.paginationController.getItems().length < visibleCount && this.paginationController.hasMore) {
            await this.paginationController.loadNextPage();
        }
    }
    
    async search(query) {
        this.searchQuery = query;
        this.paginationController.reset();
        await this.loadInitial();
    }
    
    async refresh() {
        this.paginationController.reset();
        await this.loadInitial();
    }
    
    escapeHtml(text) {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }
    
    destroy() {
        if (this.virtualScroller) {
            this.virtualScroller.destroy();
        }
    }
}