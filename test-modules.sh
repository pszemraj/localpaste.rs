#\!/bin/bash
echo "Testing LocalPaste Module Loading..."
echo "====================================="

# Test if modules are accessible
for module in "/js/editor/editor.js" "/js/utils/common.js" "/js/utils/dom-helpers.js" "/js/utils/status.js" "/js/utils/error-handler.js" "/js/api/client.js" "/js/syntax/highlighter.js" "/js/utils/virtual-scroll.js"; do
    response=$(curl -s -o /dev/null -w "%{http_code}" http://localhost:3030$module)
    if [ "$response" = "200" ]; then
        echo "✅ $module - OK"
    else
        echo "❌ $module - Failed (HTTP $response)"
    fi
done

echo ""
echo "Testing Main Application..."
echo "====================================="

# Test main page and check for console errors
response=$(curl -s http://localhost:3030/)

# Check if essential elements exist
if echo "$response" | grep -q "id=\"editor\""; then
    echo "✅ Editor element found"
else
    echo "❌ Editor element missing"
fi

if echo "$response" | grep -q "id=\"paste-list\""; then
    echo "✅ Paste list element found"
else
    echo "❌ Paste list element missing"
fi

if echo "$response" | grep -q "id=\"status-message\""; then
    echo "✅ Status element found"
else
    echo "❌ Status element missing"
fi

# Check for module imports
if echo "$response" | grep -q "import.*editor.js"; then
    echo "✅ Editor module import found"
else
    echo "❌ Editor module import missing"
fi

if echo "$response" | grep -q "import.*common.js"; then
    echo "✅ Common utils import found"
else
    echo "❌ Common utils import missing"
fi

echo ""
echo "API Endpoint Tests..."
echo "====================================="

# Test API endpoints
for endpoint in "/api/pastes" "/api/folders"; do
    response=$(curl -s -o /dev/null -w "%{http_code}" http://localhost:3030$endpoint)
    if [ "$response" = "200" ]; then
        echo "✅ $endpoint - OK"
    else
        echo "❌ $endpoint - Failed (HTTP $response)"
    fi
done

echo ""
echo "Test Complete\!"
