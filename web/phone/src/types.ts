export interface ServerMessage {
  type: 'connected' | 'error'
  device?: string
  client_id?: string
  message?: string
}

export type ClientStatus = 'disconnected' | 'connecting' | 'connected' | 'reconnecting' | 'blacklisted'