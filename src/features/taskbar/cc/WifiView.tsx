import { Check, Lock, Wifi } from "lucide-react";
import type { WifiNetwork } from "../../../lib/shell";
import { Subview } from "./Subview";

/**
 * The Wi-Fi sub-view: the network list with per-row connect actions. The
 * joining (or disconnecting) row's icon becomes a circle loader; saved and
 * open networks join straight away, unsaved secure ones go through the
 * password modal; the connected network disconnects on click.
 */
export function WifiView({
  wifiOn,
  networks,
  connecting,
  disconnecting,
  error,
  onBack,
  onOpenNetwork,
}: {
  wifiOn: boolean | null;
  networks: WifiNetwork[] | null;
  connecting: string | null;
  disconnecting: string | null;
  error: string | null;
  onBack: () => void;
  onOpenNetwork: (network: WifiNetwork) => void;
}) {
  return (
    <Subview title="Wi-Fi" onBack={onBack}>
      {wifiOn === false ? (
        <span className="cc-empty">Wi-Fi is off</span>
      ) : networks === null ? (
        <span className="cc-empty">Scanning…</span>
      ) : networks.length === 0 ? (
        <span className="cc-empty">No Wi-Fi networks found</span>
      ) : (
        <>
          {error && <span className="cc-empty cc-net-error">{error}</span>}
          {networks.map((network) => (
            <div
              className="cc-net-row cc-hit"
              key={network.ssid}
              data-connecting={
                connecting === network.ssid || disconnecting === network.ssid
              }
              onClick={() => onOpenNetwork(network)}
            >
              <span className="cc-net-circle">
                {connecting === network.ssid || disconnecting === network.ssid ? (
                  <span className="cc-net-loader" />
                ) : (
                  <Wifi size={16} className="cc-net-icon" />
                )}
              </span>
              <span className="cc-net-name" title={network.ssid}>
                {network.ssid}
              </span>
              {network.secure && <Lock size={13} className="cc-net-lock" />}
              {network.connected && <Check size={16} className="cc-net-check" />}
            </div>
          ))}
        </>
      )}
    </Subview>
  );
}
