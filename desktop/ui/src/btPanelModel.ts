// Pure formatting/selection helpers for the Bluetooth panel in the machine
// pane. Kept free of React, window and Tauri imports so Node's test runner can
// import this file directly.

export type PortLike = {
  name: string;
  description: string;
  kind?: string;
  mac?: string | null;
};

export type DeviceLike = {
  name: string;
  remembered: boolean;
  authenticated: boolean;
};

// The Rust side already spells a Bluetooth port description as
// "蓝牙串口 · <MAC>"; never append the MAC here a second time.
export function portLabel(p: PortLike): string {
  if (p.kind === "usb") return `${p.name} · CH340 ${p.description}`;
  if (p.kind === "bluetooth") return `${p.name} · ${p.description}`;
  return `${p.name} ${p.description}`;
}

export function deviceLabel(d: DeviceLike): string {
  if (d.authenticated) return `${d.name}（已配对）`;
  if (d.remembered) return `${d.name}（已记忆，未认证）`;
  return `${d.name}（未配对）`;
}

// Only a Bluetooth port whose MAC matches the one remembered from the last
// successful connection may be reconnected silently; a standby USB port must
// never be picked up by a Bluetooth-remembered session.
export function silentReconnectTarget(
  rememberedMac: string | null,
  ports: PortLike[],
): string | null {
  if (!rememberedMac) return null;
  const match = ports.find(
    (p) => p.kind === "bluetooth" && p.mac === rememberedMac,
  );
  return match ? match.name : null;
}
