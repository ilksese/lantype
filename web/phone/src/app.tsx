import { useState, useEffect, useRef, useCallback, type JSX } from 'preact/compat'
import * as stylex from '@stylexjs/stylex'
import type { ServerMessage, ClientStatus } from './types'

const pulse = stylex.keyframes({
  '0%, 100%': { opacity: 1 },
  '50%': { opacity: 0.3 },
})

const styles = stylex.create({
  body: {
    display: 'flex',
    flexDirection: 'column',
    minHeight: '100dvh',
    overflow: 'hidden',
    fontFamily:
      "-apple-system, BlinkMacSystemFont, 'Segoe UI', 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', sans-serif",
  },
  header: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    padding: '16px 20px 12px',
    borderBottom: '1px solid rgba(255,255,255,.06)',
  },
  headerLeft: {
    display: 'flex',
    alignItems: 'center',
    gap: 10,
  },
  title: {
    fontSize: 18,
    fontWeight: 600,
    letterSpacing: 0.5,
    color: '#c4b5fd',
    margin: 0,
  },
  btnSettings: {
    border: 'none',
    background: 'none',
    color: '#666',
    fontSize: 20,
    cursor: 'pointer',
    padding: 2,
    lineHeight: 1,
    transition: 'color .2s',
  },
  btnSettingsActive: {
    color: '#c4b5fd',
  },
  status: {
    display: 'flex',
    alignItems: 'center',
    gap: 6,
    fontSize: 13,
    color: '#888',
  },
  statusConnected: {
    color: '#4ade80',
  },
  statusDisconnected: {
    color: '#f87171',
  },
  statusReconnecting: {
    color: '#facc15',
  },
  dot: {
    width: 8,
    height: 8,
    borderRadius: '50%',
    background: '#555',
  },
  dotConnected: {
    background: '#4ade80',
    boxShadow: '0 0 6px rgba(74,222,128,.5)',
  },
  dotDisconnected: {
    background: '#f87171',
    boxShadow: '0 0 6px rgba(248,113,113,.5)',
  },
  dotReconnecting: {
    background: '#facc15',
    boxShadow: '0 0 6px rgba(250,204,21,.5)',
    animationName: pulse,
    animationDuration: '.8s',
    animationTimingFunction: 'ease-in-out',
    animationIterationCount: 'infinite',
  },
  deviceRow: {
    textAlign: 'center',
    padding: '10px 20px 6px',
    fontSize: 13,
    color: '#666',
    minHeight: 36,
  },
  deviceName: {
    color: '#a78bfa',
    fontWeight: 500,
  },
  inputWrap: {
    flex: 1,
    display: 'flex',
    padding: '12px 16px',
  },
  textarea: {
    flex: 1,
    width: '100%',
    resize: 'none',
    border: 'none',
    outline: 'none',
    fontSize: 22,
    lineHeight: 1.6,
    padding: '16px 18px',
    borderRadius: 16,
    background: 'rgba(255,255,255,.06)',
    color: '#eee',
    fontFamily: 'inherit',
    '::placeholder': {
      color: '#555',
    },
    ':focus': {
      background: 'rgba(255,255,255,.09)',
    },
    ':disabled': {
      opacity: 0.4,
      cursor: 'not-allowed',
    },
  },
  btnBar: {
    position: 'fixed',
    bottom: 24,
    right: 20,
    zIndex: 100,
    display: 'flex',
    gap: 10,
    alignItems: 'center',
  },
  btn: {
    border: 'none',
    borderRadius: '50%',
    width: 44,
    height: 44,
    fontSize: 16,
    cursor: 'pointer',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    boxShadow: '0 2px 8px rgba(0,0,0,.3)',
    ':active': {
      transform: 'scale(.92)',
    },
    ':disabled': {
      opacity: 0.3,
      cursor: 'not-allowed',
    },
  },
  btnSend: {
    background: '#7c3aed',
    color: '#fff',
  },
  btnClear: {
    background: 'rgba(255,255,255,.1)',
    color: '#999',
    ':active': {
      background: 'rgba(255,255,255,.2)',
      color: '#eee',
    },
  },
  btnEnter: {
    background: 'rgba(255,255,255,.15)',
    color: '#ddd',
    fontSize: 14,
    fontWeight: 600,
    ':active': {
      background: 'rgba(255,255,255,.25)',
      color: '#fff',
    },
  },
  footer: {
    textAlign: 'center',
    padding: '10px 20px 16px',
    fontSize: 11,
    color: '#444',
  },
  overlay: {
    position: 'fixed',
    top: 0,
    left: 0,
    width: '100%',
    height: '100%',
    zIndex: 200,
    background: 'rgba(0,0,0,.5)',
    display: 'none',
    alignItems: 'flex-start',
    justifyContent: 'center',
    paddingTop: 80,
  },
  overlayOpen: {
    display: 'flex',
  },
  settingsPanel: {
    background: '#262645',
    borderRadius: 16,
    width: 300,
    maxWidth: '90vw',
    padding: 20,
    boxShadow: '0 8px 32px rgba(0,0,0,.4)',
  },
  settingsTitle: {
    fontSize: 16,
    fontWeight: 600,
    color: '#c4b5fd',
    marginBottom: 16,
    margin: 0,
  },
  settingsRow: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    padding: '12px 0',
    borderTop: '1px solid rgba(255,255,255,.06)',
  },
  settingsLabel: {
    fontSize: 14,
    color: '#ccc',
  },
  toggle: {
    position: 'relative',
    width: 42,
    height: 24,
    background: '#444',
    borderRadius: 12,
    cursor: 'pointer',
    flexShrink: 0,
    '::after': {
      content: '',
      position: 'absolute',
      top: 2,
      left: 2,
      width: 20,
      height: 20,
      background: '#fff',
      borderRadius: '50%',
    },
  },
  toggleActive: {
    background: '#7c3aed',
    '::after': {
      transform: 'translateX(18px)',
    },
  },
  btnCloseSettings: {
    display: 'block',
    margin: '16px auto 0',
    border: 'none',
    background: 'rgba(255,255,255,.08)',
    color: '#999',
    borderRadius: 8,
    padding: '8px 24px',
    fontSize: 13,
    cursor: 'pointer',
    ':active': {
      background: 'rgba(255,255,255,.15)',
    },
  },
  toastWrap: {
    position: 'fixed',
    top: 0,
    left: 0,
    right: 0,
    zIndex: 300,
    display: 'flex',
    justifyContent: 'center',
    padding: '16px 20px',
    pointerEvents: 'none',
  },
  toast: {
    background: '#b91c1c',
    color: '#fff',
    borderRadius: 10,
    padding: '12px 18px',
    fontSize: 14,
    lineHeight: 1.5,
    textAlign: 'center',
    pointerEvents: 'auto',
    boxShadow: '0 4px 16px rgba(0,0,0,.4)',
    animationName: pulse,
    animationDuration: '.5s',
    animationTimingFunction: 'ease-out',
    animationIterationCount: 1,
  },
})

function getFriendlyName(): string {
  const ua = navigator.userAgent
  let browser = '浏览器'
  if (ua.includes('Chrome') && !ua.includes('Edg')) browser = 'Chrome'
  else if (ua.includes('Safari') && !ua.includes('Chrome')) browser = 'Safari'
  else if (ua.includes('Edg')) browser = 'Edge'
  else if (ua.includes('Firefox')) browser = 'Firefox'
  let os = '未知设备'
  if (ua.includes('iPhone') || ua.includes('iPad')) os = 'iOS'
  else if (ua.includes('Android')) os = 'Android'
  else if (ua.includes('Mac')) os = 'macOS'
  else if (ua.includes('Windows')) os = 'Windows'
  else if (ua.includes('Linux')) os = 'Linux'
  return browser + ' · ' + os
}

function useWebSocket() {
  const [status, setStatus] = useState<ClientStatus>('connecting')
  const [connectedDevice, setConnectedDevice] = useState<string | null>(null)
  const [errorMessage, setErrorMessage] = useState<string | null>(null)
  const wsRef = useRef<WebSocket | null>(null)
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const prevTextRef = useRef('')

  const sendMessage = useCallback((msg: object) => {
    const ws = wsRef.current
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify(msg))
    }
  }, [])

  const sendDiff = useCallback((newText: string) => {
    const prev = prevTextRef.current
    let commonLen = 0
    while (
      commonLen < prev.length &&
      commonLen < newText.length &&
      prev[commonLen] === newText[commonLen]
    ) {
      commonLen++
    }
    const backspace = prev.length - commonLen
    const append = newText.substring(commonLen)
    if (backspace > 0 || append.length > 0) {
      sendMessage({ type: 'diff', backspace, text: append })
    }
    prevTextRef.current = newText
  }, [sendMessage])

  const sendType = useCallback((text: string) => {
    sendMessage({ type: 'type', text })
  }, [sendMessage])

  const connect = useCallback(() => {
    const params = new URLSearchParams(window.location.search)
    const wsPort = params.get('ws') || '0'
    const host = window.location.hostname
    const url = 'ws://' + host + ':' + wsPort

    setStatus('reconnecting')
    const ws = new WebSocket(url)
    wsRef.current = ws

    ws.onopen = () => {
      setStatus('connected')
      ws.send(JSON.stringify({ type: 'hello', device_name: getFriendlyName() }))
    }

    ws.onmessage = (e: MessageEvent) => {
      try {
        const msg: ServerMessage = JSON.parse(e.data)
        if (msg.type === 'connected' && msg.device) {
          setConnectedDevice(msg.device)
        }
        if (msg.type === 'error' && msg.message) {
          setErrorMessage(msg.message)
        }
      } catch { /* ignore */ }
    }

    ws.onclose = (e: CloseEvent) => {
      if (e.code === 1008) {
        setStatus('blacklisted')
        return
      }
      setStatus('disconnected')
      if (timerRef.current) clearTimeout(timerRef.current)
      timerRef.current = setTimeout(connect, 3000)
    }

    ws.onerror = () => {
      ws.close()
    }
  }, [])

  useEffect(() => {
    connect()
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current)
      if (wsRef.current) wsRef.current.close()
    }
  }, [connect])

  return { status, connectedDevice, errorMessage, sendDiff, sendType: useCallback((text: string) => {
    sendMessage({ type: 'type', text })
    prevTextRef.current = ''
  }, [sendMessage]), resetPrev: useCallback(() => { prevTextRef.current = '' }, []) }
}

export function App() {
  const { status, connectedDevice, errorMessage, sendDiff, sendType, resetPrev } = useWebSocket()
  const [text, setText] = useState('')
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [autoSync, setAutoSync] = useState(false)
  const toastTimer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const isComposing = useRef(false)

  useEffect(() => {
    if (errorMessage) {
      if (toastTimer.current) clearTimeout(toastTimer.current)
      toastTimer.current = setTimeout(() => {
        // clear via reconnect/reset handled by hook
      }, 5000)
    }
    return () => {
      if (toastTimer.current) clearTimeout(toastTimer.current)
    }
  }, [errorMessage])

  const textareaRef = useRef<HTMLTextAreaElement>(null)

  const isConnected = status === 'connected'
  const hasText = text.length > 0

  const handleInput = useCallback((e: JSX.TargetedEvent<HTMLTextAreaElement>) => {
    const value = (e.target as HTMLTextAreaElement).value
    setText(value)
    if (!isComposing.current && autoSync) sendDiff(value)
  }, [autoSync, sendDiff])

  const handleCompositionStart = useCallback(() => {
    isComposing.current = true
  }, [])

  const handleCompositionEnd = useCallback((e: JSX.TargetedEvent<HTMLTextAreaElement>) => {
    isComposing.current = false
    const value = (e.target as HTMLTextAreaElement).value
    if (autoSync) sendDiff(value)
  }, [autoSync, sendDiff])

  const handleClear = useCallback(() => {
    setText('')
    sendDiff('')
    resetPrev()
    textareaRef.current?.focus()
  }, [sendDiff, resetPrev])

  const handleSend = useCallback(() => {
    if (!text) return
    sendType(text)
    setText('')
    resetPrev()
    textareaRef.current?.focus()
  }, [text, sendType, resetPrev])

  const handleEnter = useCallback(() => {
    sendType('\n')
    textareaRef.current?.focus()
  }, [sendType])

  let statusText = '未连接'
  let deviceText: string | JSX.Element = '扫码或输入地址连接'
  if (status === 'connecting' || status === 'reconnecting') {
    statusText = '连接中...'
    deviceText = '连接中...'
  } else if (status === 'connected') {
    statusText = '已连接'
    deviceText = connectedDevice
      ? <>已连接至 <span {...stylex.props(styles.deviceName)}>{connectedDevice}</span></>
      : '已连接'
  } else if (status === 'blacklisted') {
    statusText = '已被拉黑'
    deviceText = '此设备已被拉黑，无法连接'
  } else if (status === 'disconnected') {
    statusText = '已断开'
    deviceText = '连接已断开，正在重连...'
  }

  return (
    <div {...stylex.props(styles.body)}>
      {errorMessage && (
        <div {...stylex.props(styles.toastWrap)}>
          <div {...stylex.props(styles.toast)} onClick={() => {}}>
            {errorMessage}
          </div>
        </div>
      )}
      <div {...stylex.props(styles.header)}>
        <div {...stylex.props(styles.headerLeft)}>
          <h1 {...stylex.props(styles.title)}>LanType</h1>
          <button
            {...stylex.props(styles.btnSettings)}
            onClick={() => setSettingsOpen(true)}
            aria-label="设置"
          >
            &#9881;
          </button>
        </div>
        <div
          {...stylex.props(
            styles.status,
            status === 'connected' && styles.statusConnected,
            status === 'disconnected' && styles.statusDisconnected,
            (status === 'reconnecting' || status === 'connecting') && styles.statusReconnecting,
          )}
        >
          <span
            {...stylex.props(
              styles.dot,
              status === 'connected' && styles.dotConnected,
              status === 'disconnected' && styles.dotDisconnected,
              (status === 'reconnecting' || status === 'connecting') && styles.dotReconnecting,
              status === 'blacklisted' && styles.dotDisconnected,
            )}
          />
          <span>{statusText}</span>
        </div>
      </div>
      <div {...stylex.props(styles.deviceRow)}>{deviceText}</div>
      <div {...stylex.props(styles.inputWrap)}>
        <textarea
          ref={textareaRef}
          {...stylex.props(styles.textarea)}
          value={text}
          onInput={handleInput}
          onCompositionStart={handleCompositionStart}
          onCompositionEnd={handleCompositionEnd}
          placeholder="在此输入文字..."
          disabled={!isConnected && status !== 'blacklisted'}
          autocomplete="off"
          autocorrect="off"
          autocapitalize="off"
          spellcheck={false}
        />
      </div>
      <div {...stylex.props(styles.btnBar)}>
        <button
          {...stylex.props(styles.btn, styles.btnEnter)}
          onClick={handleEnter}
          disabled={!isConnected}
          aria-label="回车"
        >
          ↵
        </button>
        {(!autoSync || !hasText) && (
          <button
            {...stylex.props(styles.btn, styles.btnSend)}
            onClick={handleSend}
            disabled={!isConnected || !hasText}
            aria-label="发送"
            style={autoSync ? { display: 'none' } : undefined}
          >
            ➤
          </button>
        )}
        {hasText && (
          <button
            {...stylex.props(styles.btn, styles.btnClear)}
            onClick={handleClear}
            aria-label="清空"
          >
            ✕
          </button>
        )}
      </div>
      <div {...stylex.props(styles.footer)}>文字实时同步到桌面端</div>

      <div
        {...stylex.props(styles.overlay, settingsOpen && styles.overlayOpen)}
        onClick={(e) => { if (e.target === e.currentTarget) setSettingsOpen(false) }}
      >
        <div {...stylex.props(styles.settingsPanel)}>
          <h2 {...stylex.props(styles.settingsTitle)}>设置</h2>
          <div {...stylex.props(styles.settingsRow)}>
            <span {...stylex.props(styles.settingsLabel)}>自动同步</span>
            <div
              {...stylex.props(styles.toggle, autoSync && styles.toggleActive)}
              onClick={() => setAutoSync(!autoSync)}
            />
          </div>
          <button
            {...stylex.props(styles.btnCloseSettings)}
            onClick={() => setSettingsOpen(false)}
          >
            关闭
          </button>
        </div>
      </div>
    </div>
  )
}