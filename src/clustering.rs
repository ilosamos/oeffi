use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct StopClusterDef {
    pub key: String,
    pub name: String,
    pub member_stop_idxs: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct ClusteredStops {
    pub clusters: Vec<StopClusterDef>,
    pub cluster_idx_by_key: HashMap<String, u32>,
    pub cluster_idxs_by_name_upper: HashMap<String, Vec<u32>>,
}

pub trait ClusterStopAccessor {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn parent_station(&self) -> Option<&str>;
}

fn station_stem_from_stop_id(stop_id: &str) -> Option<String> {
    let mut parts = stop_id.split(':');
    let (Some(country), Some(region), Some(station)) = (parts.next(), parts.next(), parts.next())
    else {
        return None;
    };

    if country.is_empty() || region.is_empty() || station.is_empty() {
        return None;
    }

    Some(format!("{country}:{region}:{station}"))
}

fn station_core_from_stop_id(stop_id: &str) -> Option<String> {
    let mut parts = stop_id.split(':');
    let (_prefix, Some(region), Some(station)) = (parts.next(), parts.next(), parts.next()) else {
        return None;
    };

    if region.is_empty() || station.is_empty() {
        return None;
    }

    Some(format!("{region}:{station}"))
}

fn cluster_display_name(name: &str) -> String {
    // Keep canonical station names for output and matching (e.g. "Wien Mitte-Landstraße").
    name.trim().to_string()
}

pub fn stop_cluster_key(
    stop_id: &str,
    stop_name: &str,
    parent_station: Option<&str>,
    parent_station_ids: &HashSet<String>,
    parent_station_id_by_core: &HashMap<String, String>,
) -> String {
    // Parent-based clustering must win over stem-based clustering so parent and child stop IDs
    // (e.g. Pat:* + at:* variants in OeBB) end up in the same logical station cluster.
    if let Some(parent_id) = parent_station
        && !parent_id.is_empty()
    {
        return format!("parent::{parent_id}");
    }

    if parent_station_ids.contains(stop_id) {
        return format!("parent::{stop_id}");
    }

    if let Some(core) = station_core_from_stop_id(stop_id)
        && let Some(parent_id) = parent_station_id_by_core.get(&core)
    {
        return format!("parent::{parent_id}");
    }

    if let Some(station_stem) = station_stem_from_stop_id(stop_id) {
        return format!("stem::{station_stem}");
    }

    if !stop_name.is_empty() {
        return format!("name::{}", stop_name.to_ascii_uppercase());
    }

    format!("stop::{stop_id}")
}

pub fn build_stop_clusters<T: ClusterStopAccessor>(
    stops: &[T],
    stop_idx_by_id: &HashMap<String, u32>,
) -> ClusteredStops {
    let parent_station_ids: HashSet<String> = stops
        .iter()
        .filter_map(|stop| stop.parent_station().map(|id| id.to_string()))
        .collect();
    let mut parent_station_id_by_core: HashMap<String, String> = HashMap::new();
    for parent_id in &parent_station_ids {
        if let Some(core) = station_core_from_stop_id(parent_id) {
            parent_station_id_by_core
                .entry(core)
                .and_modify(|existing| {
                    if parent_id < existing {
                        *existing = parent_id.clone();
                    }
                })
                .or_insert_with(|| parent_id.clone());
        }
    }

    let mut clusters: Vec<StopClusterDef> = Vec::new();
    let mut cluster_idx_by_key: HashMap<String, u32> = HashMap::new();

    for (stop_idx, stop) in stops.iter().enumerate() {
        let cluster_key = stop_cluster_key(
            stop.id(),
            stop.name(),
            stop.parent_station(),
            &parent_station_ids,
            &parent_station_id_by_core,
        );

        let cluster_idx = if let Some(existing) = cluster_idx_by_key.get(&cluster_key).copied() {
            existing
        } else {
            let cluster_name = if let Some(parent_id) = cluster_key.strip_prefix("parent::") {
                stop_idx_by_id
                    .get(parent_id)
                    .and_then(|idx| stops.get(*idx as usize))
                    .map(|parent| cluster_display_name(parent.name()))
                    .unwrap_or_else(|| cluster_display_name(stop.name()))
            } else {
                cluster_display_name(stop.name())
            };

            let new_idx = clusters.len() as u32;
            cluster_idx_by_key.insert(cluster_key.clone(), new_idx);
            clusters.push(StopClusterDef {
                key: cluster_key,
                name: cluster_name,
                member_stop_idxs: Vec::new(),
            });
            new_idx
        };

        clusters[cluster_idx as usize]
            .member_stop_idxs
            .push(stop_idx as u32);
    }

    for cluster in &mut clusters {
        cluster.member_stop_idxs.sort();
        cluster.member_stop_idxs.dedup();
    }

    let mut cluster_idxs_by_name_upper: HashMap<String, Vec<u32>> = HashMap::new();
    for (idx, cluster) in clusters.iter().enumerate() {
        cluster_idxs_by_name_upper
            .entry(cluster.name.to_ascii_uppercase())
            .or_default()
            .push(idx as u32);
    }

    ClusteredStops {
        clusters,
        cluster_idx_by_key,
        cluster_idxs_by_name_upper,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::stop_cluster_key;

    #[test]
    fn stop_cluster_key_prefers_parent_station_over_stem() {
        let key = stop_cluster_key(
            "at:49:743:0:1",
            "Wien Mitte-Landstraße",
            Some("Pat:49:743"),
            &HashSet::new(),
            &HashMap::new(),
        );
        assert_eq!(key, "parent::Pat:49:743");
    }

    #[test]
    fn stop_cluster_key_uses_parent_core_mapping_for_stem_only_stop() {
        let mut by_core = HashMap::new();
        by_core.insert("49:743".to_string(), "Pat:49:743".to_string());
        let key = stop_cluster_key(
            "at:49:743:0:4",
            "Mitte-Landstraße",
            None,
            &HashSet::new(),
            &by_core,
        );
        assert_eq!(key, "parent::Pat:49:743");
    }
}
