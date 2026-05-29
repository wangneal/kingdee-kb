#!/usr/bin/env python3
"""
金蝶 COSMIC 智能问答接口调用脚本
使用 PAT Token 认证调用 /aisapi/ai-search 接口，支持流式输出

重要：原样返回接口返回的内容，不改变格式（Markdown 或 HTML）
确保图片URL完整可访问

v2.0: 去掉 kdclub-login 依赖，内置 token 存取能力
"""

import sys
import json
import argparse
import urllib.request
import urllib.parse
import urllib.error
import ssl
import re
import os
import io
import time
import stat
from pathlib import Path
from datetime import datetime
from concurrent.futures import ThreadPoolExecutor, as_completed

# ─── 编码设置 ──────────────────────────────────────────────
if os.environ.get("PYTHONIOENCODING") != "utf-8":
    os.environ["PYTHONIOENCODING"] = "utf-8"
if sys.stdout.encoding != 'utf-8':
    sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8', errors='replace', line_buffering=True)
if sys.stderr.encoding != 'utf-8':
    sys.stderr = io.TextIOWrapper(sys.stderr.buffer, encoding='utf-8', errors='replace', line_buffering=True)

# ─── Token 存储路径 ────────────────────────────────────────
TOKEN_FILE = Path.home() / '.kdclub' / 'pat_token.json'

TOKEN_NOT_FOUND_MSG = (
    "未找到有效的 PAT Token。请按以下步骤获取并提供 token：\n"
    "1. 打开浏览器访问 https://vip.kingdee.com\n"
    "2. 登录您的金蝶云社区账号\n"
    "3. 点击右上角头像 → 个人主页 → 编辑资料\n"
    "4. 找到「个人访问令牌」区域 → 新建令牌\n"
    "5. 复制生成的 token（格式如 kdt_xxxxxxxx...）\n"
    "6. 将 token 提供给我，我会自动保存供后续使用"
)


# ═══════════════════════════════════════════════════════════
#  Token 管理（内置，不依赖外部技能）
# ═══════════════════════════════════════════════════════════

def save_token(token: str):
    """
    将 token 持久化到本地文件 ~/.kdclub/pat_token.json
    同时设置当前进程环境变量，确保本次会话后续调用也能使用
    """
    TOKEN_FILE.parent.mkdir(parents=True, exist_ok=True)
    data = {
        "token": token.strip(),
        "domain": "vip.kingdee.com",
        "last_updated": datetime.now().isoformat(),
    }
    with open(TOKEN_FILE, 'w', encoding='utf-8') as f:
        json.dump(data, f, ensure_ascii=False, indent=2)

    # 设置文件权限：仅当前用户可读写
    try:
        if sys.platform == 'win32':
            os.chmod(TOKEN_FILE, stat.S_IREAD | stat.S_IWRITE)
        else:
            os.chmod(TOKEN_FILE, stat.S_IRUSR | stat.S_IWUSR)
    except Exception:
        pass

    # 同时设置进程环境变量
    os.environ['KDCLOUD_PAT_TOKEN'] = token.strip()

    print(json.dumps({
        "type": "token_saved",
        "message": f"Token 已保存到 {TOKEN_FILE}，后续会话无需重复提供。",
        "file": str(TOKEN_FILE)
    }, ensure_ascii=False), flush=True)


def load_token():
    """
    加载 PAT Token
    优先级：1. 环境变量 KDCLOUD_PAT_TOKEN → 2. 本地文件 ~/.kdclub/pat_token.json
    返回 (token, error_msg)
    """
    # 1. 环境变量
    env_token = os.environ.get("KDCLOUD_PAT_TOKEN", "").strip()
    if env_token:
        return env_token, None

    # 2. 本地文件
    if TOKEN_FILE.exists():
        try:
            with open(TOKEN_FILE, "r", encoding="utf-8") as f:
                data = json.load(f)
            token = data.get("token", "").strip()
            if token:
                # 加载到环境变量，后续本次会话内直接可用
                os.environ['KDCLOUD_PAT_TOKEN'] = token
                return token, None
        except (json.JSONDecodeError, Exception):
            pass

    # 兼容旧版 kdclub-login 的 token 文件
    legacy_paths = [
        Path.home() / '.kdclub' / 'token_vip_kingdee_com.json',
        Path.home() / '.qoderwork' / 'skills' / 'kdclub-login' / 'data' / 'token_vip_kingdee_com.json',
    ]
    for fp in legacy_paths:
        if fp.exists():
            try:
                with open(fp, "r", encoding="utf-8") as f:
                    data = json.load(f)
                token = data.get("token", "").strip()
                if token:
                    os.environ['KDCLOUD_PAT_TOKEN'] = token
                    return token, None
            except Exception:
                continue

    return None, TOKEN_NOT_FOUND_MSG


def check_token():
    """
    检查 token 是否存在且格式合法
    输出 JSON 结果到 stdout
    """
    token, err = load_token()
    if err:
        print(json.dumps({
            "type": "token_status",
            "valid": False,
            "error": err
        }, ensure_ascii=False), flush=True)
        return

    preview = f"{token[:8]}...{token[-4:]}" if len(token) > 12 else "***"
    print(json.dumps({
        "type": "token_status",
        "valid": True,
        "token_preview": preview,
        "token_length": len(token),
        "file": str(TOKEN_FILE) if TOKEN_FILE.exists() else None
    }, ensure_ascii=False), flush=True)


# ═══════════════════════════════════════════════════════════
#  工具函数
# ═══════════════════════════════════════════════════════════

def _ssl_context():
    ctx = ssl.create_default_context()
    try:
        ctx.load_default_certs()
    except Exception:
        pass
    return ctx


def _needs_title_fetch(title: str) -> bool:
    if not title:
        return True
    t = title.strip()
    if re.match(r'^\d+$', t):
        return True
    generic = [
        '点击查看完整文档', '查看完整文档', '点击查看', '查看详情',
        '点击查看详情', '详情', '文档详情', '知识详情', 'undefined', 'null',
    ]
    return t.lower() in [g.lower() for g in generic]


def fetch_page_title(url: str, token: str, timeout: int = 5) -> str:
    try:
        req = urllib.request.Request(url, headers={
            "Authorization": f"Bearer {token}",
            "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/120.0.0.0",
            "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        })
        with urllib.request.urlopen(req, timeout=timeout, context=_ssl_context()) as resp:
            html_bytes = resp.read(50 * 1024)
            for enc in ('utf-8', 'gbk', 'gb2312', 'latin-1'):
                try:
                    html = html_bytes.decode(enc)
                    break
                except (UnicodeDecodeError, LookupError):
                    continue
            else:
                html = html_bytes.decode('utf-8', errors='ignore')

            m = re.search(r'<title[^>]*>(.*?)</title>', html, re.IGNORECASE | re.DOTALL)
            if m:
                title = m.group(1).strip()
                title = title.replace('&amp;', '&').replace('&lt;', '<').replace('&gt;', '>')
                title = title.replace('&#39;', "'").replace('&quot;', '"').replace('&nbsp;', ' ')
                for suffix in [' - 金蝶云社区', ' - 金蝶社区', ' | 金蝶云社区']:
                    if title.endswith(suffix):
                        title = title[:-len(suffix)].strip()
                        break
                generic = [
                    '金蝶云社区', '金蝶社区', '金蝶云社区官网',
                    '金蝶云社区|财务金融企业信息化|IT精英人脉圈子-金蝶云社区官网',
                    '财务金融企业信息化|IT精英人脉圈子-金蝶云社区官网',
                    '点击查看完整文档', '查看完整文档', '点击查看',
                    '查看详情', '点击查看详情', '详情', '文档详情', '知识详情',
                ]
                if title in generic:
                    return ""
                if title:
                    return title
    except Exception:
        pass
    return ""


def enrich_search_sources(sources: list, token: str) -> list:
    if not sources:
        return sources
    indices = [i for i, s in enumerate(sources) if _needs_title_fetch(s.get("title", ""))]
    if not indices:
        return sources
    with ThreadPoolExecutor(max_workers=min(len(indices), 5)) as ex:
        fmap = {}
        for idx in indices:
            url = sources[idx].get("url", "")
            if url:
                fmap[ex.submit(fetch_page_title, url, token)] = idx
        for f in as_completed(fmap, timeout=10):
            try:
                t = f.result(timeout=5)
                if t:
                    sources[fmap[f]]["title"] = t
            except Exception:
                pass
    return sources


def fix_image_urls(content: str) -> str:
    """
    修复图片 URL，确保所有图片可正常展示：
    1. 相对路径补全为绝对 URL（排除 // 开头的协议相对路径）
    2. data-src 懒加载属性提升为 src（否则渲染器不加载图片）
    """
    base = "https://vip.kingdee.com"

    # ── 1. 补全相对路径（/xxx → https://vip.kingdee.com/xxx）──
    # 注意：排除 // 开头的协议相对路径
    content = re.sub(r'src="(/(?!/)[^"]+)"', lambda m: f'src="{base}{m.group(1)}"', content)
    content = re.sub(r"src='(/(?!/)[^']+)'", lambda m: f"src='{base}{m.group(1)}'", content)
    content = re.sub(r'data-src="(/(?!/)[^"]+)"', lambda m: f'data-src="{base}{m.group(1)}"', content)
    content = re.sub(r"data-src='(/(?!/)[^']+)'", lambda m: f"data-src='{base}{m.group(1)}'", content)

    # ── 2. 懒加载 data-src → src 提升 ──
    # 处理有 data-src 的 img 标签：确保 src 有真实 URL
    def _promote_data_src(match):
        tag = match.group(0)
        # 提取 data-src 的值
        ds = re.search(r'data-src=["\']([^"\']+)["\']', tag)
        if not ds:
            return tag
        real_url = ds.group(1)

        # 检查是否已有有效的 src
        src_match = re.search(r'src=["\']([^"\']*)["\']', tag)
        if src_match:
            existing_src = src_match.group(1).strip()
            # 如果 src 为空、data URI 占位符、或明显占位图，则替换
            if not existing_src or existing_src.startswith('data:') or 'placeholder' in existing_src.lower():
                tag = tag[:src_match.start()] + f'src="{real_url}"' + tag[src_match.end():]
            # else: 已有真实 src，保持不变
        else:
            # 没有 src 属性，添加一个
            tag = tag.replace('<img ', f'<img src="{real_url}" ', 1)
            tag = tag.replace('<IMG ', f'<IMG src="{real_url}" ', 1)

        return tag

    content = re.sub(r'<img\s[^>]*?data-src=[^>]*?/?>', _promote_data_src, content, flags=re.IGNORECASE)

    # ── 3. Markdown 图片相对路径补全 ──
    content = re.sub(r'!\[([^\]]*)\]\((/(?!/)[^)]+)\)', lambda m: f'![{m.group(1)}]({base}{m.group(2)})', content)

    return content


# ═══════════════════════════════════════════════════════════
#  核心：流式调用 COSMIC 问答接口
# ═══════════════════════════════════════════════════════════

def stream_cosmic(question, product_id, token,
                  session_id=None, product_line_id="35", use_deep_think=False,
                  is_retry=False):
    """
    返回值："ok" | "unauthorized" | "error"
    is_retry=True 时不输出 {"type": "start"}，避免重试时重复输出
    """
    params = {
        "scene": "1",
        "searchText": question,
        "productId": str(product_id),
        "useDeepThink": "true" if use_deep_think else "false",
        "useClarification": "false",
        "productLineId": product_line_id,
        "channel_level": "Agent Skill",
    }
    if session_id:
        params["sessionId"] = session_id

    url = "https://vip.kingdee.com/aisapi/ai-search?" + urllib.parse.urlencode(params)

    token_preview = f"{token[:8]}...{token[-4:]}" if len(token) > 12 else "***"
    sys.stderr.write(f"[DEBUG] stream_cosmic: token={token_preview}, len={len(token)}\n")
    sys.stderr.flush()

    req = urllib.request.Request(url, headers={
        "Authorization": f"Bearer {token}",
        "Accept": "text/event-stream",
        "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/120.0.0.0"
    })

    try:
        with urllib.request.urlopen(req, timeout=120, context=_ssl_context()) as response:
            full_message = ""
            think_content = ""
            final_session_id = ""
            search_sources = []
            current_step = ""

            print(json.dumps({"type": "start"}, ensure_ascii=False), flush=True) if not is_retry else None

            for line in response:
                try:
                    line = line.decode("utf-8").strip()
                except Exception:
                    continue
                if not line or not line.startswith("data:"):
                    continue
                try:
                    data = json.loads(line[5:].strip())
                except json.JSONDecodeError:
                    continue

                msg = data.get("message", "")
                is_think = data.get("isThink", False)
                step = data.get("step", "")
                if step:
                    current_step = step

                if msg == "未授权操作":
                    sys.stderr.write(f"[DEBUG] 收到'未授权操作'响应, token={token_preview}\n")
                    sys.stderr.flush()
                    return "unauthorized"

                if is_think and msg:
                    think_content += msg
                    print(json.dumps({"type": "think", "content": msg, "step": step}, ensure_ascii=False), flush=True)
                elif msg:
                    fixed_msg = fix_image_urls(msg)
                    full_message += fixed_msg
                    print(json.dumps({"type": "answer", "content": fixed_msg, "step": step}, ensure_ascii=False), flush=True)

                sid = data.get("aiSearchSessionId", "")
                if sid:
                    final_session_id = str(sid)
                src = data.get("searchSources")
                if src and isinstance(src, list) and len(src) > 0:
                    search_sources = src

                if data.get("answerEnd"):
                    # 对完整拼接后的内容再做一次图片修复（解决 SSE 分块拆断标签的问题）
                    full_message = fix_image_urls(full_message)
                    fmt = "html" if full_message.strip().startswith("<") else "markdown"
                    if search_sources:
                        search_sources = enrich_search_sources(search_sources, token)
                    print(json.dumps({
                        "type": "end", "sessionId": final_session_id,
                        "fullAnswer": full_message, "answerFormat": fmt,
                        "thinkContent": think_content, "step": current_step,
                        "searchSources": search_sources
                    }, ensure_ascii=False), flush=True)
                    return "ok"

            # 没有 answerEnd 也输出结果
            full_message = fix_image_urls(full_message)
            fmt = "html" if full_message.strip().startswith("<") else "markdown"
            if search_sources:
                search_sources = enrich_search_sources(search_sources, token)
            print(json.dumps({
                "type": "end", "sessionId": final_session_id,
                "fullAnswer": full_message, "answerFormat": fmt,
                "thinkContent": think_content, "step": current_step,
                "searchSources": search_sources
            }, ensure_ascii=False), flush=True)
            return "ok"

    except urllib.error.HTTPError as e:
        body = ""
        try:
            body = e.read().decode("utf-8")
        except Exception:
            pass
        if e.code in (401, 403):
            sys.stderr.write(f"[DEBUG] HTTP {e.code} 未授权响应, token={token_preview}\n")
            sys.stderr.flush()
            return "unauthorized"
        print(json.dumps({"type": "error", "error": f"HTTP {e.code}: {e.reason}。{body}"}, ensure_ascii=False), flush=True)
        return "error"
    except urllib.error.URLError as e:
        print(json.dumps({"type": "error", "error": f"网络请求失败: {e.reason}。请检查网络连接。"}, ensure_ascii=False), flush=True)
        return "error"
    except Exception as e:
        print(json.dumps({"type": "error", "error": f"请求异常: {str(e)}"}, ensure_ascii=False), flush=True)
        return "error"


# ═══════════════════════════════════════════════════════════
#  产品列表
# ═══════════════════════════════════════════════════════════

def load_products():
    products_file = Path(__file__).parent.parent / 'products.json'
    if not products_file.exists():
        return None, f"产品配置文件不存在: {products_file}"
    try:
        with open(products_file, 'r', encoding='utf-8') as f:
            products = json.load(f)
        if not isinstance(products, list) or len(products) == 0:
            return None, "产品配置文件内容为空或格式不正确"
        return products, None
    except json.JSONDecodeError:
        return None, f"产品配置文件格式错误: {products_file}"
    except Exception as e:
        return None, f"读取产品配置文件失败: {e}"


# ═══════════════════════════════════════════════════════════
#  主入口
# ═══════════════════════════════════════════════════════════

def main():
    parser = argparse.ArgumentParser(description="金蝶 COSMIC 智能问答（PAT Token 模式，流式输出）")
    parser.add_argument("--question", default="", help="提问内容")
    parser.add_argument("--product-id", type=int, default=0, help="产品ID")
    parser.add_argument("--token", default="", help="直接传入 PAT Token 字符串（留空则自动从本地存储读取）")
    parser.add_argument("--session-id", default="", help="多轮对话会话ID")
    parser.add_argument("--product-line-id", default="35", help="产品线ID")
    parser.add_argument("--deep-think", action="store_true", help="启用深度思考")
    parser.add_argument("--list-products", action="store_true", help="列出所有可选产品")
    parser.add_argument("--save-token", metavar="TOKEN", default="", help="保存 PAT Token 到本地（保存后退出）")
    parser.add_argument("--check-token", action="store_true", help="检查本地 Token 状态（检查后退出）")
    parser.add_argument("--init", action="store_true", help="初始化：一次性返回 Token 状态 + 产品列表（合并 check-token 和 list-products）")
    args = parser.parse_args()

    # ── 保存 token ──
    if args.save_token:
        t = args.save_token.strip()
        if not t:
            print(json.dumps({"type": "error", "error": "token 不能为空"}, ensure_ascii=False))
            sys.exit(1)
        save_token(t)
        sys.exit(0)

    # ── 检查 token ──
    if args.check_token:
        check_token()
        sys.exit(0)

    # ── 初始化（合并 check-token + list-products）──
    if args.init:
        token, err = load_token()
        if err:
            token_info = {"valid": False, "error": err}
        else:
            preview = f"{token[:8]}...{token[-4:]}" if len(token) > 12 else "***"
            token_info = {
                "valid": True,
                "token_preview": preview,
                "token_length": len(token),
                "file": str(TOKEN_FILE) if TOKEN_FILE.exists() else None
            }
        products, prod_err = load_products()
        result = {"type": "init", "token": token_info}
        if prod_err:
            result["products_error"] = prod_err
            result["products"] = []
        else:
            result["products"] = products
        print(json.dumps(result, ensure_ascii=False), flush=True)
        sys.exit(0)

    # ── 列出产品 ──
    if args.list_products:
        products, err = load_products()
        if err:
            print(json.dumps({"type": "error", "error": err}, ensure_ascii=False))
            sys.exit(1)
        print(json.dumps(products, ensure_ascii=False))
        sys.exit(0)

    # ── 问答模式 ──
    if not args.question:
        print(json.dumps({"type": "error", "error": "缺少 --question 参数"}, ensure_ascii=False))
        sys.exit(1)
    if not args.product_id:
        print(json.dumps({"type": "error", "error": "缺少 --product-id 参数"}, ensure_ascii=False))
        sys.exit(1)

    # 加载 token：--token 直传 > 环境变量 > 本地文件
    if args.token:
        token = args.token.strip()
        err = None if token else "传入的 token 为空"
    else:
        token, err = load_token()

    if err:
        print(json.dumps({"type": "error", "errorCode": "TOKEN_NOT_FOUND", "error": err}, ensure_ascii=False))
        sys.exit(1)
    if not token:
        print(json.dumps({"type": "error", "errorCode": "TOKEN_NOT_FOUND", "error": TOKEN_NOT_FOUND_MSG}, ensure_ascii=False))
        sys.exit(1)

    # 调试日志
    token_preview = f"{token[:8]}...{token[-4:]}" if len(token) > 12 else "***"
    sys.stderr.write(f"[DEBUG] main: token={token_preview}, len={len(token)}\n")
    sys.stderr.flush()

    sid = args.session_id if args.session_id else None

    # 调用接口，"未授权操作"自动重试（最多3次，间隔2秒）
    max_attempts = 3
    for attempt in range(max_attempts):
        result = stream_cosmic(
            question=args.question, product_id=args.product_id,
            token=token, session_id=sid,
            product_line_id=args.product_line_id, use_deep_think=args.deep_think,
            is_retry=(attempt > 0))

        if result != "unauthorized":
            break

        if attempt < max_attempts - 1:
            sys.stderr.write(f"[DEBUG] 未授权，2秒后重试(第{attempt+1}次)...\n")
            sys.stderr.flush()
            time.sleep(2)
            # 重新加载（以防中途刷新）
            t2, _ = load_token()
            if t2:
                token = t2
        else:
            print(json.dumps({
                "type": "error", "errorCode": "UNAUTHORIZED",
                "error": "未授权操作，PAT Token 可能已过期或无效。请重新提供有效的 token：\n"
                         "1. 访问 https://vip.kingdee.com → 个人主页 → 编辑资料 → 个人访问令牌\n"
                         "2. 复制 token 提供给我，我会自动更新保存"
            }, ensure_ascii=False), flush=True)


if __name__ == "__main__":
    main()
