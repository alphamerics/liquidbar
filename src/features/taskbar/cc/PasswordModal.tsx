import { Eye, EyeOff } from "lucide-react";
import { useEffect, useState } from "react";
import { Modal } from "glass-ui";

/**
 * The centered "join this network" password dialog. Owns the typed password
 * and the show/hide toggle; the parent decides what joining does.
 */
export function PasswordModal({
  ssid,
  busy,
  error,
  onJoin,
  onClose,
}: {
  ssid: string | null;
  busy: boolean;
  error: string | null;
  onJoin: (password: string) => void;
  onClose: () => void;
}) {
  const [password, setPassword] = useState("");
  const [visible, setVisible] = useState(false);

  // A different network gets a clean slate.
  useEffect(() => {
    setPassword("");
    setVisible(false);
  }, [ssid]);

  if (ssid === null) {
    return null;
  }

  const join = () => {
    if (!busy && password.length >= 8) {
      onJoin(password);
    }
  };

  return (
    <Modal
      open
      onClose={busy ? undefined : onClose}
      title={`Join “${ssid}”`}
      footer={
        <>
          <button
            type="button"
            className="modal-button"
            disabled={busy}
            onClick={onClose}
          >
            Cancel
          </button>
          <button
            type="button"
            className="modal-button modal-primary"
            disabled={busy || password.length < 8}
            onClick={join}
          >
            {busy ? <span className="cc-net-loader" /> : "Join"}
          </button>
        </>
      }
    >
      <div className="modal-password-row">
        <input
          autoFocus
          type={visible ? "text" : "password"}
          className="modal-input"
          placeholder="Password"
          aria-label="Wi-Fi password"
          value={password}
          onChange={(event) => setPassword(event.currentTarget.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              join();
            }
          }}
        />
        <button
          type="button"
          className="modal-eye"
          aria-label={visible ? "Hide password" : "Show password"}
          onClick={() => setVisible((current) => !current)}
        >
          {visible ? <EyeOff size={14} /> : <Eye size={14} />}
        </button>
      </div>
      {error && <span className="modal-error">{error}</span>}
    </Modal>
  );
}
