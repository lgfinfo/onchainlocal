#!/bin/bash
set -e

# 默认二进制名称
DEFAULT_BINARY_NAME="onchainlocal"
ORIGINAL_BINARY="onchainlocal"

# 获取目标二进制名称（从参数或默认值）
TARGET_BINARY="${1:-$DEFAULT_BINARY_NAME}"

# 检查参数是否为空（可选，取决于是否强制要求参数）
if [ -z "$1" ]; then
    echo "警告: 未提供目标二进制名称，使用默认值: $TARGET_BINARY"
fi

# 检测操作系统，处理 .exe 后缀
if [[ "$OSTYPE" == "msys" || "$OSTYPE" == "win32" ]]; then
    ORIGINAL_PATH="target/release/$ORIGINAL_BINARY.exe"
    TARGET_PATH="target/release/$TARGET_BINARY.exe"
else
    ORIGINAL_PATH="target/release/$ORIGINAL_BINARY"
    TARGET_PATH="target/release/$TARGET_BINARY"
fi

echo "编译项目..."
cargo build --release

echo "重命名可执行文件..."
# 检查原始二进制文件是否存在
if [ ! -f "$ORIGINAL_PATH" ]; then
    echo "错误: 原始二进制文件 $ORIGINAL_PATH 不存在"
    exit 1
fi

# 检查目标文件是否已存在
if [ -f "$TARGET_PATH" ]; then
    echo "警告: 目标文件 $TARGET_PATH 已存在，将覆盖"
    rm -f "$TARGET_PATH"
fi

# 执行重命名
mv "$ORIGINAL_PATH" "$TARGET_PATH"

echo "完成: $TARGET_PATH"

# 设置可执行权限（Linux/macOS）
if [[ "$OSTYPE" != "msys" && "$OSTYPE" != "win32" ]]; then
    chmod +x "$TARGET_PATH"
fi