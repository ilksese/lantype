export interface ServerMessage {
  type: 'connected' | 'error' | 'pong'
  device?: string
  client_id?: string
  message?: string
}

export interface Shortcut {
  id: string
  modifiers: string[]
  key: string
}

export type ClientStatus = 'disconnected' | 'connecting' | 'connected' | 'reconnecting' | 'blacklisted'
