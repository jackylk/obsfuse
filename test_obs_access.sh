#!/bin/bash
# 测试 OBS 访问的脚本

set -e

BUCKET="obs-fs-test-jska"
ENDPOINT="obs.cn-north-1.myhuaweicloud.com"

echo "测试 OBS 访问..."
echo "Bucket: $BUCKET"
echo "Endpoint: $ENDPOINT"
echo "Access Key: $OBS_ACCESS_KEY"
echo ""

# 使用 curl 测试 OBS API
URL="https://${BUCKET}.${ENDPOINT}/"

echo "尝试访问: $URL"
echo ""

# 生成签名（简化版，仅用于测试）
DATE=$(TZ=GMT date -R)

# 直接测试匿名访问
echo "1. 测试匿名访问:"
curl -s -I "$URL" | head -5
echo ""

# 测试带签名的访问需要更复杂的签名算法
echo "2. 使用提供的凭证测试 OpenDAL..."
