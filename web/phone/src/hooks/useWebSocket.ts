import { useCallback, useEffect, useRef, useState } from 'preact/compat'

export interface ServerMessage {
  type: 'ready' | 'connected' | 'ack' | 'error' | 'pending' | 'paused' | 'pong'
  request_id?: string
  sequence?: number
  device?: string
  client_id?: string
  session_id?: string
  session?: string
  message?: string
  code?: string
  retryable?: boolean
}

export interface Shortcut {
  id: string
  modifiers: string[]
  key: string
}

export type InputCommand =
  | { type: 'diff'; backspace: number; text: string }
  | { type: 'type'; text: string }
  | { type: 'keys'; modifiers: string[]; key: string }

export interface CommandAck {
  requestId: string
  sequence: number
}

export interface SendFailure extends Error {
  code: string
  retryable: boolean
  uncertain: boolean
}

export type ConnectionScheme = 'ws' | 'wss'

export interface ConnectionTarget {
  scheme: ConnectionScheme
  host: string
  port: number
  path?: string
  token?: string
  pin?: string
  session?: string
}

export interface ConnectionInput {
  link?: string
  host?: string
  port?: string | number
  credential?: string
  session?: string
}

export interface ConnectionParseResult {
  target: ConnectionTarget | null
  error: string | null
}

export type ConnectionIssueKind =
  | 'missing-target'
  | 'offline'
  | 'pairing-invalid'
  | 'approval-required'
  | 'paused'
  | 'rejected'
  | 'blacklisted'
  | 'protocol-error'
  | 'network-error'
  | 'server-error'

export interface ConnectionIssue {
  kind: ConnectionIssueKind
  code?: string
  message: string
  retryable: boolean
}

export type ClientStatus =
  | 'needs-input'
  | 'disconnected'
  | 'connecting'
  | 'connected'
  | 'reconnecting'
  | 'pending'
  | 'paused'
  | 'pairing-invalid'
  | 'rejected'
  | 'blacklisted'

interface PendingRequest {
  requestId: string
  sequence: number
  resolve: (ack: CommandAck) => void
  reject: (error: SendFailure) => void
  timer: ReturnType<typeof setTimeout>
}

const CONNECTION_TARGET_KEY = 'lantype_connection_target_v1'
const DEVICE_ID_KEY = 'lantype_device_id'
const TOKEN_MIN_LENGTH = 8
const HEARTBEAT_INTERVAL_MS = 15_000
const PONG_TIMEOUT_MS = 5_000
const RECONNECT_DELAY_MS = 3_000
const HEALTH_TIMEOUT_MS = 1_500
const READY_TIMEOUT_MS = 5_000
const COMMAND_TIMEOUT_MS = 45_000

function clean(value: string | null | undefined): string {
  return typeof value === 'string' ? value.trim() : ''
}

function defaultPort(protocol: string): number {
  return protocol === 'https:' || protocol === 'wss:' ? 443 : 80
}

function schemeForProtocol(protocol: string): ConnectionScheme {
  return protocol === 'https:' || protocol === 'wss:' ? 'wss' : 'ws'
}

function formatHost(host: string): string {
  return host.includes(':') && !host.startsWith('[') ? `[${host}]` : host
}

function normalizeHost(value: string): string {
  const host = clean(value)
  return host.startsWith('[') && host.endsWith(']') ? host.slice(1, -1) : host
}

function validHost(host: string): boolean {
  if (!host || /[\s/?#@]/.test(host)) return false
  try {
    const url = new URL(`ws://${formatHost(host)}:1`)
    return Boolean(url.hostname) && !url.username && !url.password
  } catch {
    return false
  }
}

function parsePort(value: string | number | undefined): number | null {
  const text = typeof value === 'number' ? String(value) : clean(value)
  if (!/^\d+$/.test(text)) return null
  const port = Number(text)
  return Number.isSafeInteger(port) && port <= 65_535 ? port : null
}

function validToken(value: string): boolean {
  return value.length >= TOKEN_MIN_LENGTH
    && value.length <= 1024
    && !/[\s\u0000-\u001f\u007f]/.test(value)
}

function validSession(value: string): boolean {
  return value.length > 0
    && value.length <= 1024
    && !/[\s\u0000-\u001f\u007f]/.test(value)
}

function firstParam(params: URLSearchParams, names: string[]): string {
  for (const name of names) {
    const value = clean(params.get(name))
    if (value) return value
  }
  return ''
}

function parseUrl(value: string): URL | null {
  const raw = clean(value)
  if (!raw) return null
  const candidate = /^[a-z][a-z\d+.-]*:\/\//i.test(raw) ? raw : `http://${raw}`
  try {
    return new URL(candidate)
  } catch {
    return null
  }
}

function supportedProtocol(protocol: string): boolean {
  return protocol === 'http:' || protocol === 'https:' || protocol === 'ws:' || protocol === 'wss:'
}

function sameEndpoint(left: ConnectionTarget | null, right: ConnectionTarget | null): boolean {
  if (!left || !right) return false
  return left.host === right.host && left.port === right.port && left.scheme === right.scheme
}

export function hasConnectionCredential(target: ConnectionTarget | null): boolean {
  return Boolean(target?.token || target?.pin || target?.session)
}

export function validateConnectionTarget(
  target: ConnectionTarget,
  requireCredential = true,
): string | null {
  if (!validHost(target.host)) return '主机地址无效，请填写局域网 IP 或可访问的主机名'
  if (!Number.isInteger(target.port) || target.port < 1 || target.port > 65_535) {
    return '端口需为 1-65535，不能使用 0'
  }

  if (target.pin && !/^\d{6}$/.test(target.pin)) return '配对码必须是 6 位数字'
  if (target.token && !validToken(target.token)) return `令牌至少需要 ${TOKEN_MIN_LENGTH} 个字符，且不能包含空格或控制字符`
  if (target.session && !validSession(target.session)) return '会话令牌格式无效，请重新连接桌面端'
  if (requireCredential && !hasConnectionCredential(target)) {
    return '缺少配对码或令牌，请重新扫码或向桌面端获取连接信息'
  }
  return null
}

export function isConnectableTarget(target: ConnectionTarget | null): target is ConnectionTarget {
  return Boolean(target && !validateConnectionTarget(target))
}

export function parseConnectionInput(input: ConnectionInput = {}): ConnectionParseResult {
  const rawLink = clean(input.link)
  const linkHasExplicitScheme = /^[a-z][a-z\d+.-]*:\/\//i.test(rawLink)
  let url: URL | null = null
  let scheme: ConnectionScheme = schemeForProtocol(
    typeof window !== 'undefined' ? window.location.protocol : 'http:',
  )
  let host = normalizeHost(clean(input.host))
  let portText = clean(input.port === undefined ? '' : String(input.port))
  let path = '/'
  let params = new URLSearchParams()
  let hasExplicitPort = false
  let wsPortOverride = false

  if (rawLink) {
    url = parseUrl(rawLink)
    if (!url) return { target: null, error: '链接格式无效，请输入 http(s)、ws(s) 完整地址或主机地址' }
    if (!supportedProtocol(url.protocol)) {
      return { target: null, error: '地址协议无效，请使用 http://、https://、ws:// 或 wss://' }
    }
    if (url.username || url.password) {
      return { target: null, error: '地址不能包含用户名或密码，请只填写主机和端口' }
    }

    scheme = schemeForProtocol(url.protocol)
    host = normalizeHost(url.hostname)
    hasExplicitPort = Boolean(url.port)
    portText = url.port || String(defaultPort(url.protocol))
    path = url.pathname || '/'
    params = new URLSearchParams(url.searchParams)

    const wsValue = clean(params.get('ws'))
    if (wsValue) {
      if (/^\d+$/.test(wsValue)) {
        portText = wsValue
        wsPortOverride = true
      } else {
        const wsUrl = parseUrl(wsValue)
        if (!wsUrl || !supportedProtocol(wsUrl.protocol) || !wsUrl.hostname) {
          return { target: null, error: 'ws 参数不是有效端口，请检查二维码链接' }
        }
        scheme = schemeForProtocol(wsUrl.protocol)
        host = normalizeHost(wsUrl.hostname)
        portText = wsUrl.port || String(defaultPort(wsUrl.protocol))
        path = wsUrl.pathname || '/'
        wsPortOverride = true
        const merged = new URLSearchParams(wsUrl.searchParams)
        for (const [key, value] of params) {
          if (!merged.has(key)) merged.set(key, value)
        }
        params = merged
      }
    }
  }

  if (
    (!rawLink || (!linkHasExplicitScheme && !hasExplicitPort))
    && !wsPortOverride
    && input.port !== undefined
    && String(input.port).trim()
  ) {
    portText = String(input.port).trim()
  }
  if (!host) return { target: null, error: '请填写桌面端主机地址' }
  if (!validHost(host)) return { target: null, error: '主机地址无效，请填写局域网 IP 或可访问的主机名' }

  const parsedPort = parsePort(portText)
  if (parsedPort === null) return { target: null, error: '端口需填写 1-65535 范围内的数字' }

  let token = firstParam(params, ['token', 'pairing_token'])
  const pin = firstParam(params, ['pin'])
  let session = firstParam(params, ['session', 'session_token']) || clean(input.session)
  const credential = clean(input.credential)

  // A QR URL can contain both values; the long token wins over the PIN.
  if (!token && !pin && credential) {
    if (/^\d{6}$/.test(credential)) {
      return {
        target: {
          scheme,
          host,
          port: parsedPort,
          path,
          pin: credential,
          ...(session ? { session } : {}),
        },
        error: null,
      }
    } else token = credential
  }

  const target: ConnectionTarget = {
    scheme,
    host,
    port: parsedPort,
    path,
    ...(token ? { token } : pin ? { pin } : {}),
    ...(session ? { session } : {}),
  }
  const error = validateConnectionTarget(target, false)
  return { target: error ? null : target, error }
}

export function getLocationConnectionTarget(): ConnectionTarget | null {
  if (typeof window === 'undefined') return null
  const parsed = parseConnectionInput({ link: window.location.href })
  if (parsed.target) return parsed.target
  try {
    const url = new URL(window.location.href)
    for (const key of ['token', 'pairing_token', 'pin', 'session', 'session_token']) {
      url.searchParams.delete(key)
    }
    url.searchParams.delete('ws')
    return parseConnectionInput({ link: url.toString() }).target
  } catch {
    return null
  }
}

function locationHasCredential(): boolean {
  if (typeof window === 'undefined') return false
  try {
    const params = new URL(window.location.href).searchParams
    const credentialKeys = ['token', 'pairing_token', 'pin', 'session', 'session_token']
    if (credentialKeys.some((key) => params.has(key))) return true
    const wsValue = clean(params.get('ws'))
    if (!wsValue || /^\d+$/.test(wsValue)) return false
    const wsUrl = parseUrl(wsValue)
    return Boolean(wsUrl && credentialKeys.some((key) => wsUrl.searchParams.has(key)))
  } catch {
    return false
  }
}

function normalizeStoredTarget(value: unknown): ConnectionTarget | null {
  if (!value || typeof value !== 'object') return null
  const raw = value as Partial<ConnectionTarget> & { version?: number }
  if (raw.version !== undefined && raw.version !== 1) return null
  if (raw.scheme !== 'ws' && raw.scheme !== 'wss') return null
  if (typeof raw.host !== 'string' || typeof raw.port !== 'number') return null
  const target: ConnectionTarget = {
    scheme: raw.scheme,
    host: normalizeHost(raw.host),
    port: raw.port,
    path: typeof raw.path === 'string' && raw.path.startsWith('/') ? raw.path : '/',
    ...(typeof raw.token === 'string' && raw.token ? { token: raw.token } : {}),
    ...(typeof raw.pin === 'string' && raw.pin ? { pin: raw.pin } : {}),
    ...(typeof raw.session === 'string' && raw.session ? { session: raw.session } : {}),
  }
  return validateConnectionTarget(target, true) ? null : target
}

function loadStoredTarget(): ConnectionTarget | null {
  try {
    const raw = sessionStorage.getItem(CONNECTION_TARGET_KEY)
    return raw ? normalizeStoredTarget(JSON.parse(raw)) : null
  } catch {
    return null
  }
}

function saveStoredTarget(target: ConnectionTarget): boolean {
  try {
    sessionStorage.setItem(CONNECTION_TARGET_KEY, JSON.stringify({ version: 1, ...target }))
    return true
  } catch {
    return false
  }
}

function removeStoredTarget(): void {
  try {
    sessionStorage.removeItem(CONNECTION_TARGET_KEY)
  } catch { /* ignore storage failures */ }
}

function stripCredentials(target: ConnectionTarget | null): ConnectionTarget | null {
  if (!target) return null
  return {
    scheme: target.scheme,
    host: target.host,
    port: target.port,
    path: target.path,
  }
}

function loadInitialTarget(): ConnectionTarget | null {
  const locationTarget = getLocationConnectionTarget()
  const storedTarget = loadStoredTarget()
  if (locationHasCredential()) return locationTarget
  if (storedTarget) return storedTarget
  return locationTarget
}

function requestId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') return crypto.randomUUID()
  return Date.now().toString(36) + '-' + Math.random().toString(36).slice(2)
}

function randomDeviceId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') return crypto.randomUUID()
  if (typeof crypto !== 'undefined' && typeof crypto.getRandomValues === 'function') {
    const bytes = new Uint8Array(16)
    crypto.getRandomValues(bytes)
    return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('')
  }
  return 'phone-' + Date.now().toString(36) + '-' + Math.random().toString(36).slice(2)
}

function deterministicDeviceId(): string {
  const seed = typeof navigator === 'undefined'
    ? 'lantype-phone'
    : [navigator.userAgent, navigator.language, navigator.platform].join('|')
  let hash = 2_166_136_261
  for (let index = 0; index < seed.length; index += 1) {
    hash ^= seed.charCodeAt(index)
    hash = Math.imul(hash, 16_777_619)
  }
  return `phone-fallback-${(hash >>> 0).toString(36)}`
}

function loadDeviceId(): string {
  try {
    const current = clean(localStorage.getItem(DEVICE_ID_KEY))
    if (current) return current
    const next = randomDeviceId()
    localStorage.setItem(DEVICE_ID_KEY, next)
    return next
  } catch {
    try {
      const current = clean(sessionStorage.getItem(DEVICE_ID_KEY))
      if (current) return current
      const next = randomDeviceId()
      sessionStorage.setItem(DEVICE_ID_KEY, next)
      return next
    } catch {
      return deterministicDeviceId()
    }
  }
}

function getFriendlyName(): string {
  const ua = typeof navigator !== 'undefined' ? navigator.userAgent : ''
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

function sendFailure(code: string, message: string, uncertain: boolean, retryable = true): SendFailure {
  return Object.assign(new Error(message), { code, uncertain, retryable })
}

function commandFailureIsUncertain(code: string): boolean {
  return ![
    'approval_required',
    'busy',
    'invalid_input',
    'invalid_pairing',
    'not_authorized',
    'paused',
    'payload_too_large',
    'permission_required',
  ].includes(code)
}

export function isSendFailure(value: unknown): value is SendFailure {
  return value instanceof Error
    && 'code' in value
    && 'uncertain' in value
    && typeof (value as Partial<SendFailure>).uncertain === 'boolean'
}

function defaultIssueMessage(kind: ConnectionIssueKind): string {
  switch (kind) {
    case 'missing-target': return '请填写桌面端地址和配对码，或重新扫码'
    case 'offline': return '桌面端离线或网络不可达，请确认 LanType 正在运行且手机与电脑在同一网络'
    case 'pairing-invalid': return '配对信息无效或已过期，请重新扫码或输入新的配对码'
    case 'approval-required': return '等待桌面端批准此设备，请在桌面端批准后重试连接'
    case 'paused': return '桌面端已暂停接收输入，请在桌面端恢复后重试'
    case 'rejected': return '桌面端拒绝了此设备，请重新扫码或修改连接'
    case 'blacklisted': return '此设备已被桌面端拉黑，请解除拉黑后重新配对'
    case 'protocol-error': return '桌面端返回了无法识别的协议响应，请更新桌面端后重试'
    case 'network-error': return '网络连接失败，请检查主机地址、端口和局域网连接'
    case 'server-error': return '桌面端返回了错误，请检查桌面端状态'
  }
}

function classifyServerIssue(
  codeValue: string | undefined,
  messageValue: string | undefined,
  retryable = true,
): ConnectionIssue {
  const code = clean(codeValue).toLowerCase()
  const message = clean(messageValue)
  const text = `${code} ${message.toLowerCase()}`
  let kind: ConnectionIssueKind = 'server-error'

  if (code === 'approval_required' || code === 'pending' || /approval|required approval|等待批准|待审批/.test(text)) {
    kind = 'approval-required'
  } else if (
    code === 'paused'
    || code === 'input_paused'
    || /paused|pause|接收暂停|暂停接收/.test(text)
  ) {
    kind = 'paused'
  } else if (
    code === 'invalid_pairing'
    || code === 'invalid_pin'
    || code === 'pairing_invalid'
    || /invalid (pairing|pin|token|session)|配对码无效|配对信息无效|令牌无效|会话无效/.test(text)
  ) {
    kind = 'pairing-invalid'
  } else if (code === 'blacklisted' || code === 'blocked' || /blacklist|blocked|拉黑|黑名单/.test(text)) {
    kind = 'blacklisted'
  } else if (code === 'not_authorized' || code === 'rejected' || /not authorized|rejected|拒绝|未授权/.test(text)) {
    kind = 'rejected'
  } else if (
    code.startsWith('protocol')
    || code === 'invalid_input'
    || /protocol|协议|parse error|解析/.test(text)
  ) {
    kind = 'protocol-error'
  }

  const terminal = kind === 'approval-required'
    || kind === 'paused'
    || kind === 'pairing-invalid'
    || kind === 'rejected'
    || kind === 'blacklisted'
  return {
    kind,
    ...(code ? { code } : {}),
    message: message || defaultIssueMessage(kind),
    retryable: terminal ? false : retryable,
  }
}

function classifyClose(event: CloseEvent, previous: ConnectionIssue | null): ConnectionIssue {
  const offline = (message = defaultIssueMessage('offline')): ConnectionIssue => ({
    kind: 'offline',
    code: event.code ? `close_${event.code}` : 'connection_closed',
    message,
    retryable: true,
  })

  if (previous?.kind === 'approval-required') {
    return offline('审批连接已断开，正在重新发起审批')
  }
  if (previous && (
    previous.kind === 'paused'
    || previous.kind === 'pairing-invalid'
    || previous.kind === 'rejected'
    || previous.kind === 'blacklisted'
  )) return previous

  const reason = clean(event.reason).toLowerCase()
  if (event.code === 1008) {
    if (/heartbeat.*timeout|pong.*timeout|心跳.*超时/.test(reason)) return offline()
    if (/approval.*(pending|timeout)|pending.*approval|审批.*(等待|超时)/.test(reason)) {
      return offline('审批连接已断开，正在重新发起审批')
    }
    if (/pause|暂停/.test(reason)) return { kind: 'paused', code: 'paused', message: defaultIssueMessage('paused'), retryable: false }
    if (/pin|pair|token|session|配对|令牌/.test(reason)) return { kind: 'pairing-invalid', code: 'invalid_pairing', message: defaultIssueMessage('pairing-invalid'), retryable: false }
    if (/block|blacklist|拉黑|黑名单/.test(reason)) return { kind: 'blacklisted', code: 'blacklisted', message: defaultIssueMessage('blacklisted'), retryable: false }
    if (/protocol|hello|unsupported|invalid input|协议|握手/.test(reason)) return { kind: 'protocol-error', code: 'protocol_rejected', message: defaultIssueMessage('protocol-error'), retryable: false }
    return {
      kind: 'rejected',
      code: 'policy_violation',
      message: reason ? `${defaultIssueMessage('rejected')}（${reason}）` : defaultIssueMessage('rejected'),
      retryable: false,
    }
  }
  if (event.code === 1002 || event.code === 1003 || event.code === 1007) {
    return { kind: 'protocol-error', code: `close_${event.code}`, message: reason || defaultIssueMessage('protocol-error'), retryable: false }
  }
  return offline()
}

function clearCredentialParamsFromUrl(): void {
  if (typeof window === 'undefined' || !window.history?.replaceState) return
  try {
    const url = new URL(window.location.href)
    const sensitive = new Set(['token', 'pairing_token', 'pin', 'session', 'session_token'])
    for (const key of Array.from(url.searchParams.keys())) {
      if (sensitive.has(key.toLowerCase())) url.searchParams.delete(key)
    }
    const wsValue = clean(url.searchParams.get('ws'))
    if (wsValue && !/^\d+$/.test(wsValue)) {
      const wsUrl = parseUrl(wsValue)
      if (wsUrl && supportedProtocol(wsUrl.protocol)) {
        for (const key of Array.from(wsUrl.searchParams.keys())) {
          if (sensitive.has(key.toLowerCase())) wsUrl.searchParams.delete(key)
        }
        url.searchParams.set('ws', wsUrl.toString())
      }
    }
    const next = url.pathname + (url.search ? url.search : '') + (url.hash || '')
    window.history.replaceState(window.history.state, '', next)
  } catch { /* history can be unavailable in embedded browsers */ }
}

function targetUrl(target: ConnectionTarget): string {
  const host = formatHost(target.host)
  const url = new URL(`${target.scheme}://${host}:${target.port}${target.path || '/'}`)
  if (target.token) url.searchParams.set('token', target.token)
  else if (target.pin) url.searchParams.set('pin', target.pin)
  if (target.session) {
    url.searchParams.set('session', target.session)
    url.searchParams.set('session_token', target.session)
  }
  return url.toString()
}

async function diagnoseReachability(
  target: ConnectionTarget,
  fallback: ConnectionIssue,
): Promise<ConnectionIssue> {
  if (fallback.kind !== 'offline' || typeof fetch !== 'function') return fallback
  const protocol = target.scheme === 'wss' ? 'https:' : 'http:'
  const url = new URL(`${protocol}//${formatHost(target.host)}:${target.port}/health`)
  const sameOrigin = typeof window !== 'undefined' && url.origin === window.location.origin
  const controller = new AbortController()
  const timer = setTimeout(() => controller.abort(), HEALTH_TIMEOUT_MS)
  try {
    const response = await fetch(url, {
      cache: 'no-store',
      credentials: 'omit',
      mode: sameOrigin ? 'same-origin' : 'no-cors',
      signal: controller.signal,
    })
    if (response.type !== 'opaque') {
      try {
        const value: unknown = await response.json()
        if (value && typeof value === 'object' && (
          (value as { paused?: unknown }).paused === true
          || (value as { input_paused?: unknown }).input_paused === true
        )) {
          return {
            kind: 'paused',
            code: 'paused',
            message: defaultIssueMessage('paused'),
            retryable: false,
          }
        }
      } catch { /* reachability is enough when an older health response has no JSON state */ }
    }
    return {
      kind: 'network-error',
      code: 'websocket_unavailable',
      message: '桌面端可以访问，但 WebSocket 连接失败，请检查配对信息、协议或网络代理',
      retryable: true,
    }
  } catch {
    return fallback
  } finally {
    clearTimeout(timer)
  }
}

export function useWebSocket(nicknameRef: { current: string }) {
  const initialTargetRef = useRef<ConnectionTarget | null | undefined>(undefined)
  if (initialTargetRef.current === undefined) initialTargetRef.current = loadInitialTarget()
  const initialTarget = initialTargetRef.current
  const initialStatus: ClientStatus = isConnectableTarget(initialTarget) ? 'connecting' : 'needs-input'

  const [connectionTarget, setConnectionTarget] = useState<ConnectionTarget | null>(initialTarget)
  const [status, setStatus] = useState<ClientStatus>(initialStatus)
  const [connectedDevice, setConnectedDevice] = useState<string | null>(null)
  const [errorMessage, setErrorMessage] = useState<string | null>(null)
  const [connectionIssue, setConnectionIssue] = useState<ConnectionIssue | null>(null)
  const [pendingCount, setPendingCount] = useState(0)
  const [connectionEpoch, setConnectionEpoch] = useState(0)
  const wsRef = useRef<WebSocket | null>(null)
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const heartbeatRef = useRef<ReturnType<typeof setInterval> | null>(null)
  const pongTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const resumeHeartbeatRef = useRef<() => void>(() => {})
  const pendingRequestsRef = useRef(new Map<string, PendingRequest>())
  const sequenceRef = useRef(0)
  const readyRef = useRef(false)
  const targetRef = useRef<ConnectionTarget | null>(initialTarget)
  const statusRef = useRef<ClientStatus>(initialStatus)
  const issueRef = useRef<ConnectionIssue | null>(null)
  const reconnectBlockedRef = useRef(!isConnectableTarget(initialTarget))
  const deviceIdRef = useRef(loadDeviceId())

  const setStatusValue = useCallback((next: ClientStatus) => {
    statusRef.current = next
    setStatus(next)
  }, [])

  const setIssueValue = useCallback((next: ConnectionIssue | null) => {
    issueRef.current = next
    setConnectionIssue(next)
    setErrorMessage(next?.message ?? null)
  }, [])

  const clearReconnectTimer = useCallback(() => {
    if (!reconnectTimerRef.current) return
    clearTimeout(reconnectTimerRef.current)
    reconnectTimerRef.current = null
  }, [])

  const clearHeartbeat = useCallback(() => {
    if (heartbeatRef.current) {
      clearInterval(heartbeatRef.current)
      heartbeatRef.current = null
    }
    if (pongTimeoutRef.current) {
      clearTimeout(pongTimeoutRef.current)
      pongTimeoutRef.current = null
    }
  }, [])

  const takePending = useCallback((id?: string): PendingRequest | null => {
    const requests = pendingRequestsRef.current
    const resolvedId = id ?? (requests.size === 1 ? requests.keys().next().value : undefined)
    if (typeof resolvedId !== 'string') return null
    const pending = requests.get(resolvedId)
    if (!pending) return null
    clearTimeout(pending.timer)
    requests.delete(resolvedId)
    setPendingCount(requests.size)
    if (requests.size === 0) resumeHeartbeatRef.current()
    return pending
  }, [])

  const resolvePending = useCallback((id?: string) => {
    const pending = takePending(id)
    if (!pending) return
    setErrorMessage(null)
    pending.resolve({ requestId: pending.requestId, sequence: pending.sequence })
  }, [takePending])

  const rejectPending = useCallback((failure: SendFailure, id?: string) => {
    const pending = takePending(id)
    if (pending) pending.reject(failure)
  }, [takePending])

  const rejectAllPending = useCallback((failure: SendFailure) => {
    const requests = pendingRequestsRef.current
    for (const pending of requests.values()) {
      clearTimeout(pending.timer)
      pending.reject(failure)
    }
    requests.clear()
    setPendingCount(0)
    resumeHeartbeatRef.current()
  }, [])

  const closeCurrentSocket = useCallback(() => {
    const ws = wsRef.current
    wsRef.current = null
    if (!ws) return
    ws.onopen = null
    ws.onmessage = null
    ws.onclose = null
    ws.onerror = null
    if (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING) ws.close()
  }, [])

  const sendCommand = useCallback((command: InputCommand): Promise<CommandAck> => {
    const ws = wsRef.current
    if (!readyRef.current || !ws || ws.readyState !== WebSocket.OPEN) {
      const failure = sendFailure('not_connected', '连接尚未就绪，草稿已保留', false)
      setErrorMessage(failure.message)
      return Promise.reject(failure)
    }
    if (pendingRequestsRef.current.size > 0) {
      const failure = sendFailure('client_busy', '上一条命令仍在等待确认', false)
      setErrorMessage(failure.message)
      return Promise.reject(failure)
    }

    const id = requestId()
    const sequence = ++sequenceRef.current

    return new Promise<CommandAck>((resolve, reject) => {
      const pending: PendingRequest = {
        requestId: id,
        sequence,
        resolve,
        reject,
        timer: setTimeout(() => {
          const failure = sendFailure('ack_timeout', '发送确认超时，请检查桌面端实际结果', true)
          rejectPending(failure, id)
        }, COMMAND_TIMEOUT_MS),
      }

      pendingRequestsRef.current.set(id, pending)
      setPendingCount(pendingRequestsRef.current.size)
      clearHeartbeat()
      try {
        ws.send(JSON.stringify({ ...command, request_id: id, sequence }))
      } catch {
        const failure = sendFailure('send_failed', '消息未能写入连接，草稿已保留', false)
        setErrorMessage(failure.message)
        rejectPending(failure, id)
      }
    })
  }, [clearHeartbeat, rejectPending])

  const connect = useCallback((force = false): boolean => {
    const target = targetRef.current
    if (!isConnectableTarget(target)) {
      clearReconnectTimer()
      reconnectBlockedRef.current = true
      setStatusValue('needs-input')
      return false
    }
    if (reconnectBlockedRef.current) return false

    const current = wsRef.current
    if (!force && current && (
      current.readyState === WebSocket.OPEN || current.readyState === WebSocket.CONNECTING
    )) return true

    clearReconnectTimer()
    clearHeartbeat()
    readyRef.current = false
    resumeHeartbeatRef.current = () => {}
    if (pendingRequestsRef.current.size > 0) {
      rejectAllPending(sendFailure(
        'connection_reset',
        '连接已重置，上一条命令是否执行尚不确定',
        true,
      ))
    }
    closeCurrentSocket()
    setConnectedDevice(null)
    setIssueValue(null)
    const wasInitial = statusRef.current === 'connecting' || statusRef.current === 'needs-input'
    setStatusValue(wasInitial ? 'connecting' : 'reconnecting')

    let url: string
    try {
      url = targetUrl(target)
    } catch {
      const issue: ConnectionIssue = {
        kind: 'network-error',
        code: 'invalid_target',
        message: '连接地址无效，请修改主机和端口后重试',
        retryable: false,
      }
      reconnectBlockedRef.current = true
      setStatusValue('disconnected')
      setIssueValue(issue)
      return false
    }

    let ws: WebSocket
    try {
      ws = new WebSocket(url)
    } catch {
      const issue: ConnectionIssue = {
        kind: 'network-error',
        code: 'websocket_constructor',
        message: '无法创建 WebSocket 连接，请检查地址和端口',
        retryable: false,
      }
      reconnectBlockedRef.current = true
      setStatusValue('disconnected')
      setIssueValue(issue)
      return false
    }

    wsRef.current = ws
    let announcedReady = false
    let terminalIssue: ConnectionIssue | null = null
    let readyTimer: ReturnType<typeof setTimeout> | null = null

    const clearReadyTimer = () => {
      if (!readyTimer) return
      clearTimeout(readyTimer)
      readyTimer = null
    }

    const applyTerminalIssue = (issue: ConnectionIssue) => {
      terminalIssue = issue
      reconnectBlockedRef.current = true
      clearReconnectTimer()
      clearReadyTimer()
      readyRef.current = false
      setIssueValue(issue)
      if (issue.kind === 'approval-required') setStatusValue('pending')
      else if (issue.kind === 'paused') setStatusValue('paused')
      else if (issue.kind === 'pairing-invalid') setStatusValue('pairing-invalid')
      else if (issue.kind === 'blacklisted') setStatusValue('blacklisted')
      else if (issue.kind === 'rejected') setStatusValue('rejected')
      else setStatusValue('disconnected')
      const failure = sendFailure(issue.code || issue.kind, issue.message, false, false)
      if (pendingRequestsRef.current.size > 0) rejectAllPending(failure)
    }

    const applyApprovalPending = (issue: ConnectionIssue) => {
      terminalIssue = null
      reconnectBlockedRef.current = false
      clearReadyTimer()
      readyRef.current = false
      setIssueValue(issue)
      setStatusValue('pending')
    }

    const markReady = (message: ServerMessage) => {
      clearReadyTimer()
      terminalIssue = null
      const sessionId = clean(message.session_id || message.session)
      if (sessionId && !validSession(sessionId)) {
        applyTerminalIssue({
          kind: 'protocol-error',
          code: 'invalid_session_id',
          message: '桌面端返回的 session_id 无效，请更新桌面端后重试',
          retryable: false,
        })
        return
      }

      readyRef.current = true
      reconnectBlockedRef.current = false
      setStatusValue('connected')
      setIssueValue(null)
      if (message.device) setConnectedDevice(message.device)

      const currentTarget = targetRef.current
      if (currentTarget) {
        const nextTarget = sessionId ? { ...currentTarget, session: sessionId } : currentTarget
        targetRef.current = nextTarget
        setConnectionTarget(nextTarget)
        if (saveStoredTarget(nextTarget)) clearCredentialParamsFromUrl()
      }

      if (!announcedReady) {
        announcedReady = true
        sequenceRef.current = 0
        setConnectionEpoch((value) => value + 1)
      }
    }

    const sendPing = () => {
      if (typeof document !== 'undefined' && document.visibilityState === 'hidden') return
      if (wsRef.current !== ws || ws.readyState !== WebSocket.OPEN) return
      if (pendingRequestsRef.current.size > 0) {
        if (pongTimeoutRef.current) {
          clearTimeout(pongTimeoutRef.current)
          pongTimeoutRef.current = null
        }
        return
      }
      try {
        ws.send(JSON.stringify({ type: 'ping' }))
        if (pongTimeoutRef.current) clearTimeout(pongTimeoutRef.current)
        pongTimeoutRef.current = setTimeout(() => {
          if (wsRef.current === ws && ws.readyState === WebSocket.OPEN) ws.close()
        }, PONG_TIMEOUT_MS)
      } catch {
        ws.close()
      }
    }

    const startHeartbeat = () => {
      clearHeartbeat()
      if (
        wsRef.current !== ws
        || ws.readyState !== WebSocket.OPEN
        || pendingRequestsRef.current.size > 0
        || reconnectBlockedRef.current
      ) return
      heartbeatRef.current = setInterval(sendPing, HEARTBEAT_INTERVAL_MS)
      sendPing()
    }

    resumeHeartbeatRef.current = startHeartbeat

    const sendHelloFrame = () => {
      const currentTarget = targetRef.current
      const hello = {
        type: 'hello',
        protocol_version: 2,
        device_id: deviceIdRef.current,
        device_name: nicknameRef.current || getFriendlyName(),
        ...(currentTarget?.token ? { pairing_token: currentTarget.token } : {}),
        ...(currentTarget?.session ? { session_token: currentTarget.session } : {}),
      }
      ws.send(JSON.stringify(hello))
    }

    ws.onopen = () => {
      if (wsRef.current !== ws) return
      readyTimer = setTimeout(() => {
        readyTimer = null
        if (wsRef.current !== ws || readyRef.current || terminalIssue) return
        applyTerminalIssue({
          kind: 'protocol-error',
          code: 'ready_timeout',
          message: '桌面端未返回 ready 响应，请更新桌面端后重试',
          retryable: false,
        })
        ws.close(1002, 'ready timeout')
      }, READY_TIMEOUT_MS)
      try {
        sendHelloFrame()
      } catch {
        ws.close()
        return
      }
      startHeartbeat()
    }

    ws.onmessage = (event: MessageEvent) => {
      if (wsRef.current !== ws || typeof event.data !== 'string') return
      try {
        const value: unknown = JSON.parse(event.data)
        if (!value || typeof value !== 'object' || !('type' in value)) return
        const message = value as ServerMessage

        if (message.type === 'pong') {
          if (pongTimeoutRef.current) {
            clearTimeout(pongTimeoutRef.current)
            pongTimeoutRef.current = null
          }
          return
        }

        if (message.type === 'ready') {
          markReady(message)
          return
        }

        if (message.type === 'connected') {
          if (message.device) setConnectedDevice(message.device)
          return
        }

        if (message.type === 'ack') {
          resolvePending(message.request_id)
          return
        }

        if (message.type === 'pending') {
          if (!message.request_id) {
            applyApprovalPending(classifyServerIssue('approval_required', message.message, true))
          }
          return
        }

        if (message.type === 'paused') {
          applyTerminalIssue(classifyServerIssue('paused', message.message, false))
          return
        }

        if (message.type === 'error') {
          const issue = classifyServerIssue(message.code, message.message, message.retryable ?? true)
          if (issue.kind === 'approval-required' && message.request_id === undefined) {
            applyApprovalPending(issue)
            return
          }
          const failure = sendFailure(
            issue.code || 'server_error',
            issue.message,
            commandFailureIsUncertain(issue.code || 'server_error'),
            issue.retryable,
          )
          if (
            issue.kind === 'paused'
            || issue.kind === 'pairing-invalid'
            || issue.kind === 'rejected'
            || issue.kind === 'blacklisted'
            || (message.request_id === undefined && (
              issue.kind === 'protocol-error' || !issue.retryable
            ))
          ) {
            applyTerminalIssue(issue)
          } else {
            terminalIssue = null
            setErrorMessage(issue.message)
            rejectPending(failure, message.request_id)
          }
        }
      } catch {
        const issue: ConnectionIssue = {
          kind: 'protocol-error',
          code: 'malformed_message',
          message: defaultIssueMessage('protocol-error'),
          retryable: false,
        }
        applyTerminalIssue(issue)
        ws.close()
      }
    }

    ws.onclose = (event: CloseEvent) => {
      if (wsRef.current !== ws) return
      wsRef.current = null
      resumeHeartbeatRef.current = () => {}
      clearReadyTimer()
      readyRef.current = false
      clearHeartbeat()
      if (pendingRequestsRef.current.size > 0) {
        rejectAllPending(sendFailure(
          'connection_lost',
          '连接中断，上一条命令是否执行尚不确定',
          true,
        ))
      }

      if (terminalIssue) {
        applyTerminalIssue(terminalIssue)
        return
      }

      const issue = classifyClose(event, issueRef.current)
      if (!issue.retryable) {
        applyTerminalIssue(issue)
        return
      }

      setStatusValue('disconnected')
      setIssueValue(issue)
      clearReconnectTimer()
      reconnectTimerRef.current = setTimeout(() => {
        reconnectTimerRef.current = null
        if (!reconnectBlockedRef.current && isConnectableTarget(targetRef.current)) connect(true)
      }, RECONNECT_DELAY_MS)
      void diagnoseReachability(target, issue).then((diagnosed) => {
        if (wsRef.current || terminalIssue || !sameEndpoint(targetRef.current, target)) return
        if (diagnosed.kind === 'paused') applyTerminalIssue(diagnosed)
        else setIssueValue(diagnosed)
      })
    }

    ws.onerror = () => {
      if (wsRef.current === ws) ws.close()
    }
    return true
  }, [
    clearHeartbeat,
    clearReconnectTimer,
    closeCurrentSocket,
    nicknameRef,
    rejectAllPending,
    rejectPending,
    resolvePending,
    setIssueValue,
    setStatusValue,
  ])

  const connectWithTarget = useCallback((next: ConnectionTarget): boolean => {
    const normalized: ConnectionTarget = {
      ...next,
      host: normalizeHost(next.host),
      path: next.path?.startsWith('/') ? next.path : '/',
    }
    const validationError = validateConnectionTarget(normalized)
    if (validationError) {
      setStatusValue('needs-input')
      setIssueValue({
        kind: 'missing-target',
        code: 'invalid_target',
        message: validationError,
        retryable: false,
      })
      return false
    }
    targetRef.current = normalized
    setConnectionTarget(normalized)
    reconnectBlockedRef.current = false
    setIssueValue(null)
    return connect(true)
  }, [connect, setIssueValue, setStatusValue])

  const retryConnection = useCallback((): boolean => {
    const target = targetRef.current
    if (!isConnectableTarget(target)) {
      setStatusValue('needs-input')
      return false
    }
    reconnectBlockedRef.current = false
    setIssueValue(null)
    return connect(true)
  }, [connect, setIssueValue, setStatusValue])

  const editConnection = useCallback((clearCredentials = false) => {
    clearReconnectTimer()
    clearHeartbeat()
    readyRef.current = false
    reconnectBlockedRef.current = true
    resumeHeartbeatRef.current = () => {}
    if (pendingRequestsRef.current.size > 0) {
      rejectAllPending(sendFailure('connection_reset', '连接已停止，上一条命令是否执行尚不确定', true, false))
    }
    closeCurrentSocket()
    setConnectedDevice(null)

    const next = clearCredentials ? stripCredentials(targetRef.current) : targetRef.current
    targetRef.current = next
    setConnectionTarget(next)
    if (clearCredentials) {
      removeStoredTarget()
      clearCredentialParamsFromUrl()
    }
    setIssueValue(null)
    setStatusValue(isConnectableTarget(next) ? 'disconnected' : 'needs-input')
  }, [clearHeartbeat, clearReconnectTimer, closeCurrentSocket, rejectAllPending, setIssueValue, setStatusValue])

  const ensureConnected = useCallback(() => {
    if (typeof document !== 'undefined' && document.visibilityState === 'hidden') return
    if (reconnectBlockedRef.current || !isConnectableTarget(targetRef.current)) return
    if (pendingRequestsRef.current.size > 0) {
      if (pongTimeoutRef.current) {
        clearTimeout(pongTimeoutRef.current)
        pongTimeoutRef.current = null
      }
      return
    }
    const ws = wsRef.current
    if (ws && ws.readyState === WebSocket.CONNECTING) return
    if (!ws || ws.readyState !== WebSocket.OPEN) {
      connect(true)
      return
    }
    try {
      ws.send(JSON.stringify({ type: 'ping' }))
      if (pongTimeoutRef.current) clearTimeout(pongTimeoutRef.current)
      pongTimeoutRef.current = setTimeout(() => {
        if (wsRef.current === ws) connect(true)
      }, PONG_TIMEOUT_MS)
    } catch {
      connect(true)
    }
  }, [connect])

  useEffect(() => {
    if (isConnectableTarget(targetRef.current)) connect()
    else setStatusValue('needs-input')
    return () => {
      clearReconnectTimer()
      clearHeartbeat()
      readyRef.current = false
      resumeHeartbeatRef.current = () => {}
      rejectAllPending(sendFailure('unmounted', '页面已关闭', true, false))
      closeCurrentSocket()
    }
  }, [clearHeartbeat, clearReconnectTimer, closeCurrentSocket, connect, rejectAllPending, setStatusValue])

  useEffect(() => {
    const onVisible = () => {
      if (typeof document !== 'undefined' && document.visibilityState === 'visible') ensureConnected()
    }
    const onPageShow = () => ensureConnected()

    document.addEventListener('visibilitychange', onVisible)
    window.addEventListener('pageshow', onPageShow)
    return () => {
      document.removeEventListener('visibilitychange', onVisible)
      window.removeEventListener('pageshow', onPageShow)
    }
  }, [ensureConnected])

  const sendHello = useCallback(() => {
    const ws = wsRef.current
    if (!ws || ws.readyState !== WebSocket.OPEN) return false
    const target = targetRef.current
    try {
      ws.send(JSON.stringify({
        type: 'hello',
        protocol_version: 2,
        device_id: deviceIdRef.current,
        device_name: nicknameRef.current || getFriendlyName(),
        ...(target?.token ? { pairing_token: target.token } : {}),
        ...(target?.session ? { session_token: target.session } : {}),
      }))
      return true
    } catch {
      return false
    }
  }, [nicknameRef])

  return {
    status,
    connectedDevice,
    errorMessage,
    connectionIssue,
    connectionTarget,
    needsConnectionInput: !isConnectableTarget(connectionTarget),
    pendingCount,
    connectionEpoch,
    sendCommand,
    sendHello,
    connectWithTarget,
    retryConnection,
    editConnection,
  }
}
