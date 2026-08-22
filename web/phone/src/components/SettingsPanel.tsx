import type { JSX } from 'preact/compat'
import styles from './SettingsPanel.module.css'

function cx(...names: (string | false | null | undefined)[]): string {
  return names.filter(Boolean).join(' ')
}

interface SettingsPanelProps {
  open: boolean
  nickname: string
  keepAwake: boolean
  wakeLockSupported: boolean
  onNickname: (e: JSX.TargetedEvent<HTMLInputElement>) => void
  onKeepAwake: (enabled: boolean) => void
  onClose: () => void
}

export function SettingsPanel({
  open,
  nickname,
  keepAwake,
  wakeLockSupported,
  onNickname,
  onKeepAwake,
  onClose,
}: SettingsPanelProps) {
  return (
    <div
      className={cx(styles.overlay, open && styles.overlayOpen)}
      onClick={(e) => { if (e.target === e.currentTarget) onClose() }}
    >
      <div className={styles.settingsPanel}>
        <h2 className={styles.settingsTitle}>设置</h2>
        <div className={styles.settingsRow}>
          <span className={styles.settingsLabel}>昵称</span>
          <input
            className={styles.nicknameInput}
            value={nickname}
            onInput={onNickname}
            placeholder="显示在接收端设备列表"
            maxLength={20}
          />
        </div>
        <div className={styles.settingsRow}>
          <div className={styles.settingsText}>
            <span className={styles.settingsLabel}>保持常亮</span>
            {!wakeLockSupported && <span className={styles.settingsNote}>当前浏览器不支持</span>}
          </div>
          <button
            className={cx(styles.switch, keepAwake && styles.switchOn)}
            onClick={() => onKeepAwake(!keepAwake)}
            disabled={!wakeLockSupported}
            role="switch"
            aria-checked={keepAwake}
            type="button"
          >
            <span className={cx(styles.switchKnob, keepAwake && styles.switchKnobOn)} />
          </button>
        </div>
        <button
          className={styles.btnCloseSettings}
          onClick={onClose}
        >
          关闭
        </button>
      </div>
    </div>
  )
}
