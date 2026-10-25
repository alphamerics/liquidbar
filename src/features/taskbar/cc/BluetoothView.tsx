import { Bluetooth, Check } from "lucide-react";
import type { BluetoothDevice } from "../../../lib/shell";
import { Subview } from "./Subview";

/**
 * The Bluetooth sub-view: every device Windows knows, paired or just seen
 * nearby. Clicking an unpaired device pairs it (Windows may pop its own
 * confirmation panel for passkey devices); clicking a paired device unpairs
 * it. The clicked row's icon becomes a circle loader until it settles.
 */
export function BluetoothView({
  devices,
  busy,
  error,
  onBack,
  onDevice,
}: {
  devices: BluetoothDevice[] | null;
  busy: string | null;
  error: string | null;
  onBack: () => void;
  onDevice: (device: BluetoothDevice) => void;
}) {
  return (
    <Subview title="Bluetooth" onBack={onBack}>
      {devices === null ? (
        <span className="cc-empty">Scanning…</span>
      ) : devices.length === 0 ? (
        <span className="cc-empty">No Bluetooth devices found</span>
      ) : (
        <>
          {error && <span className="cc-empty cc-net-error">{error}</span>}
          {devices.map((device) => (
            <div
              className="cc-net-row cc-hit"
              key={device.id}
              data-connecting={busy === device.id}
              onClick={() => onDevice(device)}
            >
              <span className="cc-net-circle">
                {busy === device.id ? (
                  <span className="cc-net-loader" />
                ) : (
                  <Bluetooth size={15} className="cc-net-icon" />
                )}
              </span>
              <span className="cc-net-name" title={device.name}>
                {device.name}
              </span>
              {device.connected && <Check size={16} className="cc-net-check" />}
              <span className="cc-net-signal">
                {device.connected
                  ? "Connected"
                  : device.paired
                    ? "Paired"
                    : "Not paired"}
              </span>
            </div>
          ))}
        </>
      )}
    </Subview>
  );
}
