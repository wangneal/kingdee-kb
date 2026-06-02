/**
 * 大纲节点组件
 *
 * 单个大纲节点的完整交互：选择、展开/收起、内联编辑、
 * 键盘导航、完成状态切换、语音按钮占位。
 */
import { useState, useRef, useCallback, useEffect, useMemo } from "react";
import {
  ChevronRight,
  ChevronDown,
  GripVertical,
  CheckCircle,
  Circle,
  Mic,
  Trash2,
} from "lucide-react";
import { useOutline, type TreeNode } from "../../contexts/OutlineContext";
import { useAudio } from "../../contexts/AudioContext";

interface OutlineNodeProps {
  node: TreeNode;
  depth: number;
}

export default function OutlineNode({ node, depth }: OutlineNodeProps) {
  const {
    tree,
    selectedNodeId,
    expandedNodeIds,
    selectNode,
    toggleExpand,
    createNode,
    updateNode,
    deleteNode,
    moveNode,
  } = useOutline();

  const { status, startAudioRecording, stopAudioRecording } = useAudio();
  const [editing, setEditing] = useState(false);
  const [editContent, setEditContent] = useState(node.content);
  const [isRecording, setRecording] = useState(false);
  const contentRef = useRef<HTMLDivElement>(null);

  const isSelected = selectedNodeId === node.id;
  const isExpanded = expandedNodeIds.has(node.id);
  const hasChildren = node.children.length > 0;

  // 同步外部内容变化
  useEffect(() => {
    if (!editing) {
      setEditContent(node.content);
    }
  }, [node.content, editing]);

  // ── 辅助：获取可见节点的扁平列表（用于键盘导航） ──
  const getVisibleNodeIds = useCallback((): number[] => {
    const ids: number[] = [];
    const walk = (nodes: TreeNode[]) => {
      for (const n of nodes) {
        ids.push(n.id);
        if (expandedNodeIds.has(n.id) && n.children.length > 0) {
          walk(n.children);
        }
      }
    };
    walk(tree);
    return ids;
  }, [tree, expandedNodeIds]);

  // ── 选择 ──
  const handleClick = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation();
      selectNode(node.id);
    },
    [node.id, selectNode],
  );

  // ── 展开/收起 ──
  const handleToggle = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation();
      toggleExpand(node.id);
    },
    [node.id, toggleExpand],
  );

  // ── 双击进入编辑模式 ──
  const handleDoubleClick = useCallback(() => {
    setEditing(true);
    setEditContent(node.content);
    // 延迟聚焦，等待 contentEditable 渲染
    requestAnimationFrame(() => {
      if (contentRef.current) {
        contentRef.current.focus();
        // 选中全部文本
        const range = document.createRange();
        range.selectNodeContents(contentRef.current);
        const sel = window.getSelection();
        sel?.removeAllRanges();
        sel?.addRange(range);
      }
    });
  }, [node.content]);

  // ── 保存编辑 ──
  const saveEdit = useCallback(async () => {
    const trimmed = editContent.trim();
    if (trimmed !== node.content) {
      await updateNode(node.id, { content: trimmed });
    }
    setEditing(false);
  }, [editContent, node.content, node.id, updateNode]);

  // ── 取消编辑 ──
  const cancelEdit = useCallback(() => {
    setEditContent(node.content);
    setEditing(false);
  }, [node.content]);

  // ── 完成状态切换 ──
  const handleToggleCompleted = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation();
      updateNode(node.id, { completed: !node.completed });
    },
    [node.id, node.completed, updateNode],
  );

  // ── 删除节点 ──
  const handleDelete = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation();
      if (window.confirm(`确认删除节点"${node.content}"？${hasChildren ? "子节点也将被删除。" : ""}`)) {
        deleteNode(node.id);
      }
    },
    [node.id, node.content, hasChildren, deleteNode],
  );

  // ── 语音输入：开始/停止录音并追加转录文本 ──
  const handleVoice = useCallback(
    async (e: React.MouseEvent) => {
      e.stopPropagation();
      if (status === "idle" || status === "error") {
        await startAudioRecording();
        setRecording(true);
      } else if (status === "recording") {
        setRecording(false);
        const text = await stopAudioRecording();
        if (text) {
          const newContent = node.content ? `${node.content}\n${text}` : text;
          await updateNode(node.id, { content: newContent });
        }
      }
    },
    [status, startAudioRecording, stopAudioRecording, node.content, node.id, updateNode],
  );

  // ── 编辑模式键盘处理 ──
  const handleEditKeyDown = useCallback(
    async (e: React.KeyboardEvent) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        await saveEdit();
        // 创建同级节点
        const newNode = await createNode(node.parent_id, "");
        if (newNode) {
          selectNode(newNode.id);
        }
      } else if (e.key === "Escape") {
        e.preventDefault();
        cancelEdit();
      } else if (e.key === "Tab") {
        e.preventDefault();
        await saveEdit();
        if (e.shiftKey) {
          // Shift+Tab: 提升层级（成为父节点的兄弟）
          if (node.parent_id !== null) {
            const parentNode = tree.find((n) => n.id === node.parent_id);
            if (parentNode) {
              await moveNode(node.id, parentNode.parent_id, parentNode.sort_order + 1);
            }
          }
        } else {
          // Tab: 缩进（成为上一个兄弟的子节点）
          const siblings = tree.filter((n) => n.parent_id === node.parent_id);
          const idx = siblings.findIndex((n) => n.id === node.id);
          if (idx > 0) {
            const prevSibling = siblings[idx - 1];
            await moveNode(node.id, prevSibling.id, prevSibling.children.length);
          }
        }
      }
    },
    [saveEdit, cancelEdit, createNode, node, selectNode, moveNode, tree],
  );

  // ── 导航模式键盘处理 ──
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (editing) return; // 编辑模式下不处理导航键

      const visibleIds = getVisibleNodeIds();
      const currentIdx = visibleIds.indexOf(node.id);

      switch (e.key) {
        case "ArrowUp": {
          e.preventDefault();
          if (currentIdx > 0) {
            selectNode(visibleIds[currentIdx - 1]);
          }
          break;
        }
        case "ArrowDown": {
          e.preventDefault();
          if (currentIdx < visibleIds.length - 1) {
            selectNode(visibleIds[currentIdx + 1]);
          }
          break;
        }
        case "ArrowLeft": {
          e.preventDefault();
          if (isExpanded && hasChildren) {
            toggleExpand(node.id);
          }
          break;
        }
        case "ArrowRight": {
          e.preventDefault();
          if (!isExpanded && hasChildren) {
            toggleExpand(node.id);
          }
          break;
        }
        case "Enter": {
          e.preventDefault();
          // 创建同级节点
          createNode(node.parent_id, "").then((newNode) => {
            if (newNode) selectNode(newNode.id);
          });
          break;
        }
        case "Tab": {
          e.preventDefault();
          if (e.shiftKey) {
            // Shift+Tab: 提升层级
            if (node.parent_id !== null) {
              const parentNode = tree.find((n) => n.id === node.parent_id);
              if (parentNode) {
                moveNode(node.id, parentNode.parent_id, parentNode.sort_order + 1);
              }
            }
          } else {
            // Tab: 缩进
            const siblings = tree.filter((n) => n.parent_id === node.parent_id);
            const idx = siblings.findIndex((n) => n.id === node.id);
            if (idx > 0) {
              const prevSibling = siblings[idx - 1];
              moveNode(node.id, prevSibling.id, prevSibling.children.length);
            }
          }
          break;
        }
        case "Delete":
        case "Backspace": {
          e.preventDefault();
          if (window.confirm(`确认删除节点"${node.content}"？${hasChildren ? "子节点也将被删除。" : ""}`)) {
            deleteNode(node.id);
          }
          break;
        }
      }
    },
    [editing, node, isExpanded, hasChildren, getVisibleNodeIds, selectNode, toggleExpand, createNode, moveNode, deleteNode, tree],
  );

  // ── 选中时自动聚焦（接收键盘事件） ──
  const containerRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (isSelected && !editing && containerRef.current) {
      containerRef.current.focus();
    }
  }, [isSelected, editing]);

  // ── 解析标签 ──
  const tags = useMemo(() => {
    try {
      return JSON.parse(node.tags) as string[];
    } catch {
      return [];
    }
  }, [node.tags]);

  return (
    <div>
      {/* 节点行 */}
      <div
        ref={containerRef}
        role="treeitem"
        aria-selected={isSelected}
        aria-expanded={hasChildren ? isExpanded : undefined}
        tabIndex={isSelected ? 0 : -1}
        onClick={handleClick}
        onKeyDown={handleKeyDown}
        className={`group flex items-center gap-1 rounded-md px-2 py-1 text-sm transition-colors outline-none ${
          isSelected
            ? "bg-[#1A6BD8]/10 text-[#1A6BD8]"
            : "text-neutral-700 hover:bg-neutral-100"
        } ${node.completed ? "opacity-60" : ""}`}
        style={{ paddingLeft: `${depth * 20 + 8}px` }}
      >
        {/* 展开/收起按钮 */}
        {hasChildren ? (
          <button
            type="button"
            onClick={handleToggle}
            className="flex h-4 w-4 shrink-0 items-center justify-center rounded text-neutral-400 hover:text-neutral-600 transition-colors"
          >
            {isExpanded ? (
              <ChevronDown className="h-3.5 w-3.5" />
            ) : (
              <ChevronRight className="h-3.5 w-3.5" />
            )}
          </button>
        ) : (
          <span className="h-4 w-4 shrink-0" />
        )}

        {/* 拖拽手柄（仅视觉，暂无 DnD） */}
        <GripVertical className="h-3.5 w-3.5 shrink-0 cursor-grab text-neutral-300 opacity-0 group-hover:opacity-100 transition-opacity" />

        {/* 内容区域 */}
        {editing ? (
          <div
            ref={contentRef}
            contentEditable
            suppressContentEditableWarning
            className={`min-w-0 flex-1 rounded border border-[#1A6BD8] bg-white px-1.5 py-0.5 text-sm outline-none ${
              node.completed ? "line-through text-neutral-400" : ""
            }`}
            onInput={(e) => setEditContent((e.target as HTMLDivElement).textContent ?? "")}
            onKeyDown={handleEditKeyDown}
            onBlur={saveEdit}
            dangerouslySetInnerHTML={{ __html: editContent }}
          />
        ) : (
          <span
            className={`min-w-0 flex-1 cursor-text truncate ${
              node.completed ? "line-through text-neutral-400" : ""
            }`}
            onDoubleClick={handleDoubleClick}
          >
            {node.content || <span className="italic text-neutral-300">空节点</span>}
          </span>
        )}

        {/* 标签 */}
        {tags.length > 0 && (
          <div className="flex shrink-0 gap-1">
            {tags.map((tag) => (
              <span
                key={tag}
                className="rounded bg-blue-50 px-1.5 py-0.5 text-[10px] text-blue-600"
              >
                {tag}
              </span>
            ))}
          </div>
        )}

        {/* 完成状态按钮 */}
        <button
          type="button"
          onClick={handleToggleCompleted}
          className="shrink-0 rounded p-0.5 text-neutral-300 hover:text-green-500 transition-colors"
          title={node.completed ? "标记为未完成" : "标记为已完成"}
        >
          {node.completed ? (
            <CheckCircle className="h-4 w-4 text-green-500" />
          ) : (
            <Circle className="h-4 w-4" />
          )}
        </button>

        {/* 语音按钮 */}
        <button
          type="button"
          onClick={handleVoice}
          disabled={status === "transcribing"}
          className={`shrink-0 rounded p-0.5 transition-all ${
            isRecording
              ? "text-red-500 opacity-100 animate-pulse"
              : status === "transcribing"
                ? "text-amber-400 opacity-100"
                : "text-neutral-300 opacity-0 group-hover:opacity-100 hover:text-[#1A6BD8]"
          }`}
          title={
            isRecording
              ? "点击停止录音"
              : status === "transcribing"
                ? "转录中..."
                : "语音输入"
          }
        >
          <Mic className="h-3.5 w-3.5" />
        </button>

        {/* 删除按钮 */}
        <button
          type="button"
          onClick={handleDelete}
          className="shrink-0 rounded p-0.5 text-neutral-300 opacity-0 group-hover:opacity-100 hover:text-red-500 transition-all"
          title="删除节点"
        >
          <Trash2 className="h-3.5 w-3.5" />
        </button>
      </div>

      {/* 子节点（递归渲染） */}
      {hasChildren && isExpanded && (
        <div role="group">
          {node.children.map((child) => (
            <OutlineNode key={child.id} node={child} depth={depth + 1} />
          ))}
        </div>
      )}
    </div>
  );
}
