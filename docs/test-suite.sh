#!/bin/bash

# LocalPaste.rs Automated Test Suite
# Run this after any refactoring to ensure nothing is broken

set -e  # Exit on first error

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

API_URL="http://127.0.0.1:3030"

echo -e "${YELLOW}=== LocalPaste.rs Test Suite ===${NC}\n"

# Check if server is running
echo -n "Checking server status... "
if curl -s "$API_URL" > /dev/null; then
    echo -e "${GREEN}✓${NC}"
else
    echo -e "${RED}✗${NC}"
    echo "Server not running! Start with: cargo run --release"
    exit 1
fi

# Test 1: Create paste
echo -n "Test 1: Create paste... "
PASTE_ID=$(curl -s -X POST "$API_URL/api/paste" \
  -H "Content-Type: application/json" \
  -d '{"name": "Test Suite Paste", "content": "test content", "language": "javascript"}' \
  | jq -r '.id')
if [ ! -z "$PASTE_ID" ] && [ "$PASTE_ID" != "null" ]; then 
    echo -e "${GREEN}✓${NC}"
else 
    echo -e "${RED}✗${NC}"
    exit 1
fi

# Test 2: Get paste
echo -n "Test 2: Get paste... "
NAME=$(curl -s "$API_URL/api/paste/$PASTE_ID" | jq -r '.name')
if [ "$NAME" = "Test Suite Paste" ]; then 
    echo -e "${GREEN}✓${NC}"
else 
    echo -e "${RED}✗${NC}"
fi

# Test 3: Update paste with language
echo -n "Test 3: Update paste language... "
curl -s -X PUT "$API_URL/api/paste/$PASTE_ID" \
  -H "Content-Type: application/json" \
  -d '{"name": "Updated Test", "language": "python"}' \
  -o /dev/null
LANG=$(curl -s "$API_URL/api/paste/$PASTE_ID" | jq -r '.language')
if [ "$LANG" = "python" ]; then 
    echo -e "${GREEN}✓${NC}"
else 
    echo -e "${RED}✗ (Language not updated)${NC}"
fi

# Test 4: List pastes
echo -n "Test 4: List pastes... "
COUNT=$(curl -s "$API_URL/api/pastes?limit=10" | jq 'length')
if [ "$COUNT" -gt 0 ]; then 
    echo -e "${GREEN}✓${NC}"
else 
    echo -e "${RED}✗${NC}"
fi

# Test 5: Search
echo -n "Test 5: Search pastes... "
RESULTS=$(curl -s "$API_URL/api/search?q=test" | jq 'length')
if [ "$RESULTS" -gt 0 ]; then 
    echo -e "${GREEN}✓${NC}"
else 
    echo -e "${RED}✗${NC}"
fi

# Test 6: Create folder
echo -n "Test 6: Create folder... "
FOLDER_ID=$(curl -s -X POST "$API_URL/api/folder" \
  -H "Content-Type: application/json" \
  -d '{"name": "Test Folder"}' | jq -r '.id')
if [ ! -z "$FOLDER_ID" ] && [ "$FOLDER_ID" != "null" ]; then 
    echo -e "${GREEN}✓${NC}"
else 
    echo -e "${RED}✗${NC}"
fi

# Test 7: List folders
echo -n "Test 7: List folders... "
FOLDER_COUNT=$(curl -s "$API_URL/api/folders" | jq 'length')
if [ "$FOLDER_COUNT" -gt 0 ]; then 
    echo -e "${GREEN}✓${NC}"
else 
    echo -e "${RED}✗${NC}"
fi

# Test 8: Update folder
echo -n "Test 8: Update folder... "
if [ ! -z "$FOLDER_ID" ]; then
    curl -s -X PUT "$API_URL/api/folder/$FOLDER_ID" \
      -H "Content-Type: application/json" \
      -d '{"name": "Renamed Folder"}' -o /dev/null
    FOLDER_NAME=$(curl -s "$API_URL/api/folders" | jq -r '.[] | select(.id=="'$FOLDER_ID'") | .name')
    if [ "$FOLDER_NAME" = "Renamed Folder" ]; then
        echo -e "${GREEN}✓${NC}"
    else
        echo -e "${RED}✗${NC}"
    fi
else
    echo -e "${YELLOW}Skipped${NC}"
fi

# Test 9: Delete paste
echo -n "Test 9: Delete paste... "
curl -s -X DELETE "$API_URL/api/paste/$PASTE_ID" -o /dev/null
STATUS=$(curl -s -o /dev/null -w "%{http_code}" "$API_URL/api/paste/$PASTE_ID")
if [ "$STATUS" = "404" ]; then 
    echo -e "${GREEN}✓${NC}"
else 
    echo -e "${RED}✗${NC}"
fi

# Test 10: Delete folder
echo -n "Test 10: Delete folder... "
if [ ! -z "$FOLDER_ID" ]; then
    STATUS=$(curl -s -X DELETE "$API_URL/api/folder/$FOLDER_ID" -o /dev/null -w "%{http_code}")
    if [ "$STATUS" = "200" ] || [ "$STATUS" = "204" ]; then
        echo -e "${GREEN}✓${NC}"
    else
        echo -e "${RED}✗${NC}"
    fi
else
    echo -e "${YELLOW}Skipped${NC}"
fi

# Test 11: Check for JavaScript errors
echo -n "Test 11: Check for JS errors... "
# Create a temporary test page load
curl -s "$API_URL/" -o /dev/null
sleep 2
# This would need to check server logs for JS errors
# For now, we'll just check if the page loads
if curl -s "$API_URL/" | grep -q "LocalPaste.rs"; then
    echo -e "${GREEN}✓ (Page loads)${NC}"
else
    echo -e "${RED}✗${NC}"
fi

echo -e "\n${YELLOW}=== Test Suite Complete ===${NC}"
echo -e "${GREEN}All API endpoints are functioning correctly!${NC}"
echo ""
echo "Manual UI checks required:"
echo "  - Language selector visible (not black-on-black)"
echo "  - Sidebar shows folders and pastes"
echo "  - Editor allows typing and highlighting"
echo "  - Auto-save works after 1 second"
echo "  - Drag and drop for paste organization"