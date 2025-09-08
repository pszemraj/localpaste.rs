# LocalPaste.rs Refactoring Guide

**CRITICAL: READ THIS BEFORE ANY FRONTEND REFACTORING**

This document captures the working state after the successful incremental refactoring completed on 2025-09-08.

## Current Working Architecture

### The Golden Rule
**DO NOT attempt to replace the entire LocalPaste class at once!** This has been tried and it breaks everything. The UI currently works with a hybrid approach where LocalPaste coordinates modular components.

### Current State (Working)
- **Main Class**: `LocalPaste` in `index.html` (~1600 lines but WORKING)
- **API Client**: All API calls use `this.api` (ApiClient instance)
- **Modules**: Located in `/js/` directory, loaded dynamically
- **State**: LocalPaste manages state, modules assist with functionality

## Critical Components That Must Remain

### 1. LocalPaste Constructor Requirements
```javascript
class LocalPaste {
    constructor() {
        // REQUIRED: API client initialization
        this.api = window.ApiClient ? new window.ApiClient() : null;
        
        // REQUIRED: Debounced save function
        this.debouncedSave = this.debounce(() => this.savePaste(), 1000);
        
        // REQUIRED: Call init()
        this.init();
    }
    
    // REQUIRED: Debounce implementation
    debounce(func, wait) {
        let timeout;
        return function executedFunction(...args) {
            const later = () => {
                clearTimeout(timeout);
                func(...args);
            };
            clearTimeout(timeout);
            timeout = setTimeout(later, wait);
        };
    }
}
```

### 2. Module Loading Pattern
Modules are loaded dynamically and made available globally:
```javascript
// Pattern that works:
import('/js/api/client.js').then(module => {
    window.ApiClient = module.ApiClient;
});

// Then used in LocalPaste:
this.api = window.ApiClient ? new window.ApiClient() : null;
```

### 3. CSS Requirements
Language selector MUST have explicit styling to avoid black-on-black:
```css
#paste-language { 
    background: var(--bg-tertiary); 
    color: var(--text-primary); 
}
#paste-language option { 
    background: var(--bg-tertiary); 
    color: var(--text-primary); 
}
```

## Working Module Structure

```
src/static/
├── index.html (main app with LocalPaste class)
├── css/
│   └── style.css (with language selector fixes)
└── js/
    ├── api/
    │   └── client.js (ApiClient class - WORKING)
    ├── components/
    │   ├── App.js (created but NOT YET INTEGRATED)
    │   ├── Editor.js (created but NOT YET INTEGRATED)
    │   └── Sidebar.js (created but NOT YET INTEGRATED)
    ├── state/
    │   └── store.js (EventTarget-based state management)
    ├── syntax/
    │   ├── highlighter.js (with inline escapeHtml)
    │   ├── language-patterns.js (pattern definitions)
    │   ├── languages.js (for Web Worker)
    │   └── worker-highlighter.js (Web Worker wrapper)
    ├── utils/
    │   ├── common.js
    │   ├── dom-helpers.js
    │   ├── error-handler.js
    │   ├── logger.js (ConsoleReporter)
    │   └── virtual-scroll.js
    └── workers/
        └── highlight-worker.js (Web Worker for syntax highlighting)
```

## Tested and Working Features

All of these have been verified working as of the last commit:

### API Operations (via ApiClient)
- ✅ Create paste: `this.api.createPaste({ content: '' })`
- ✅ Get paste: `this.api.getPaste(id)`
- ✅ Update paste: `this.api.updatePaste(id, updates)`
- ✅ Delete paste: `this.api.deletePaste(id)`
- ✅ List pastes: `this.api.listPastes(limit, offset)`
- ✅ Search: `this.api.searchPastes(query)`
- ✅ Create folder: `this.api.createFolder({ name })`
- ✅ List folders: `this.api.listFolders()`
- ✅ Update folder: `this.api.updateFolder(id, { name })`
- ✅ Delete folder: `this.api.deleteFolder(id)`

### UI Features
- ✅ Sidebar with folders and paste list
- ✅ Editor with syntax highlighting
- ✅ Language selector (properly styled)
- ✅ Auto-save with 1000ms debounce
- ✅ Drag and drop for organizing pastes
- ✅ Search functionality
- ✅ Keyboard shortcuts

## Common Pitfalls to Avoid

### ❌ DON'T DO THIS:
1. **Don't remove LocalPaste class entirely** - The app breaks
2. **Don't try to use ES6 imports in index.html** - Use dynamic imports
3. **Don't forget the debounce function** - Causes "is not a function" errors
4. **Don't remove inline styles from language selector** - Causes black-on-black
5. **Don't use `import { escapeHtml } from '../utils/dom.js'`** - File is `dom-helpers.js`

### ✅ DO THIS INSTEAD:
1. **Keep LocalPaste as coordinator** while gradually delegating to modules
2. **Use dynamic imports** with global assignment pattern
3. **Always define debounce** in the LocalPaste class
4. **Keep CSS overrides** for select elements
5. **Use inline functions** or correct import paths

## Incremental Migration Strategy

If you need to continue refactoring:

### Phase 1: Current State (COMPLETED)
- LocalPaste class handles everything
- ApiClient module handles all API calls
- Modules loaded but not fully utilized

### Phase 2: Gradual Delegation (SAFE APPROACH)
```javascript
// Keep LocalPaste but delegate specific functions
class LocalPaste {
    async savePaste() {
        // Instead of inline code, delegate to module
        if (window.EditorModule) {
            return window.EditorModule.save(this.currentPaste);
        }
        // Fallback to existing implementation
    }
}
```

### Phase 3: Component Integration (CAREFUL)
- Keep LocalPaste as the app shell
- Mount components inside LocalPaste.init()
- Let components handle their specific areas
- LocalPaste remains as the coordinator

### Phase 4: Full Migration (DANGEROUS)
- Only attempt after Phase 3 is stable
- Must maintain ALL existing functionality
- Test extensively at each step

## Testing Checklist

Before considering any refactor complete, verify:

```bash
# 1. Build succeeds
cargo build --release

# 2. No unwrap() panics in production code
grep -r "unwrap()" src/*.rs | grep -v test

# 3. Server starts without errors
cargo run --release

# 4. No JavaScript errors in console
curl -s http://127.0.0.1:3030/ && check server logs

# 5. All API endpoints work
./test-suite.sh  # Create this from the tests above

# 6. UI features work:
- [ ] Create new paste
- [ ] Edit paste content
- [ ] Change language (check styling!)
- [ ] Save (auto-save after 1 second)
- [ ] Search pastes
- [ ] Create folder
- [ ] Organize pastes in folders
- [ ] Delete paste
- [ ] Delete folder
```

## Recovery Instructions

If you break the UI during refactoring:

```bash
# 1. Restore the last known working commit
git checkout c683445  # Last verified working state

# 2. Or restore just index.html
git checkout c683445 -- src/static/index.html

# 3. Rebuild and restart
cargo build --release
pkill -f localpaste
cargo run --release

# 4. Verify it works
curl -s http://127.0.0.1:3030/api/pastes
```

## Key Commits for Reference

- `c683445` - Last verified fully working state with all fixes
- `40662e4` - ApiClient fully integrated
- `31e316f` - Module loading fixes
- `a78cbf3` - CSS fix for language selector
- `926c2c6` - Last state before major refactor attempt

## Final Notes

The current implementation is not perfect but it WORKS. Any refactoring must:
1. Be done incrementally
2. Be tested after each change
3. Maintain backward compatibility
4. Not break existing functionality

Remember: **A working monolith is better than a broken modular architecture!**

---
*Document created: 2025-09-08*
*Last known working commit: c683445*