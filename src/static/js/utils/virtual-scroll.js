/**
 * Virtual scrolling implementation for efficient rendering of large lists
 * Only renders items that are visible in the viewport
 */
export class VirtualScroller {
    constructor(options) {
        this.container = options.container;
        this.itemHeight = options.itemHeight || 32;
        this.items = options.items || [];
        this.renderItem = options.renderItem;
        this.overscan = options.overscan || 3;
        
        this.scrollTop = 0;
        this.containerHeight = 0;
        this.visibleStart = 0;
        this.visibleEnd = 0;
        
        this.scrollElement = null;
        this.contentElement = null;
        this.spacerTop = null;
        this.spacerBottom = null;
        
        this.init();
    }
    
    init() {
        this.scrollElement = document.createElement('div');
        this.scrollElement.className = 'virtual-scroll-container';
        this.scrollElement.style.cssText = `
            height: 100%;
            overflow-y: auto;
            position: relative;
        `;
        
        this.spacerTop = document.createElement('div');
        this.spacerBottom = document.createElement('div');
        
        this.contentElement = document.createElement('div');
        this.contentElement.className = 'virtual-scroll-content';
        
        this.scrollElement.appendChild(this.spacerTop);
        this.scrollElement.appendChild(this.contentElement);
        this.scrollElement.appendChild(this.spacerBottom);
        
        this.container.innerHTML = '';
        this.container.appendChild(this.scrollElement);
        
        this.scrollElement.addEventListener('scroll', this.handleScroll.bind(this));
        
        const resizeObserver = new ResizeObserver(() => {
            this.containerHeight = this.scrollElement.clientHeight;
            this.update();
        });
        resizeObserver.observe(this.scrollElement);
        
        this.containerHeight = this.scrollElement.clientHeight;
        this.update();
    }
    
    handleScroll() {
        this.scrollTop = this.scrollElement.scrollTop;
        this.update();
    }
    
    setItems(items) {
        this.items = items;
        this.scrollElement.scrollTop = 0;
        this.scrollTop = 0;
        this.update();
    }
    
    update() {
        const totalHeight = this.items.length * this.itemHeight;
        
        const visibleStart = Math.floor(this.scrollTop / this.itemHeight);
        const visibleEnd = Math.ceil((this.scrollTop + this.containerHeight) / this.itemHeight);
        
        const renderStart = Math.max(0, visibleStart - this.overscan);
        const renderEnd = Math.min(this.items.length, visibleEnd + this.overscan);
        
        if (renderStart === this.visibleStart && renderEnd === this.visibleEnd) {
            return;
        }
        
        this.visibleStart = renderStart;
        this.visibleEnd = renderEnd;
        
        const topSpacerHeight = renderStart * this.itemHeight;
        const bottomSpacerHeight = (this.items.length - renderEnd) * this.itemHeight;
        
        this.spacerTop.style.height = `${topSpacerHeight}px`;
        this.spacerBottom.style.height = `${bottomSpacerHeight}px`;
        
        this.contentElement.innerHTML = '';
        
        for (let i = renderStart; i < renderEnd; i++) {
            const item = this.items[i];
            const element = this.renderItem(item, i);
            if (element) {
                element.style.height = `${this.itemHeight}px`;
                this.contentElement.appendChild(element);
            }
        }
    }
    
    scrollToItem(index) {
        const targetScrollTop = index * this.itemHeight;
        this.scrollElement.scrollTop = targetScrollTop;
    }
    
    destroy() {
        if (this.scrollElement) {
            this.scrollElement.removeEventListener('scroll', this.handleScroll);
        }
        if (this.container) {
            this.container.innerHTML = '';
        }
    }
}

/**
 * Pagination helper for loading data in chunks
 */
export class PaginationController {
    constructor(options) {
        this.pageSize = options.pageSize || 50;
        this.loadPage = options.loadPage;
        this.onDataLoaded = options.onDataLoaded;
        
        this.currentPage = 0;
        this.totalPages = 0;
        this.allItems = [];
        this.hasMore = true;
        this.isLoading = false;
    }
    
    async loadNextPage() {
        if (this.isLoading || !this.hasMore) {
            return;
        }
        
        this.isLoading = true;
        
        try {
            const items = await this.loadPage(this.currentPage, this.pageSize);
            
            if (items.length < this.pageSize) {
                this.hasMore = false;
            }
            
            this.allItems = [...this.allItems, ...items];
            this.currentPage++;
            
            if (this.onDataLoaded) {
                this.onDataLoaded(this.allItems);
            }
            
            return items;
        } finally {
            this.isLoading = false;
        }
    }
    
    async loadAll() {
        while (this.hasMore) {
            await this.loadNextPage();
        }
        return this.allItems;
    }
    
    reset() {
        this.currentPage = 0;
        this.allItems = [];
        this.hasMore = true;
        this.isLoading = false;
    }
    
    getItems() {
        return this.allItems;
    }
}