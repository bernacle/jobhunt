import type { PreferenceControls, PreferenceView } from "./api-types";

/** The ids of every record a structured control shows, so the rest can be listed apart. */
export function controlRecordIds(controls: PreferenceControls): Set<string> {
  const records: (PreferenceView | null | undefined)[] = [
    ...controls.work.setup_records,
    controls.work.relocation_record,
    controls.location.home_record,
    ...controls.location.authorized_in.map((p) => p.record),
    ...controls.location.remote_geography.map((p) => p.record),
    controls.location.unclear_eligibility_record,
    ...controls.pay.minimum.map((p) => p.record),
    ...controls.pay.target.map((p) => p.record),
    controls.pay.unknown_pay_record,
    ...controls.company.items.map((i) => i.record),
  ];
  return new Set(records.filter((r): r is PreferenceView => Boolean(r)).map((r) => r.id));
}
