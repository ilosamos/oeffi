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
    pub stop_idx_to_cluster_idx: Vec<u32>,
}

pub trait ClusterStopAccessor {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn parent_station(&self) -> Option<&str>;
}

pub fn stop_cluster_key(
    stop_id: &str,
    stop_name: &str,
    parent_station: Option<&str>,
    parent_station_ids: &HashSet<String>,
) -> String {
    if let Some(parent_id) = parent_station {
        if !parent_id.is_empty() {
            return format!("parent::{parent_id}");
        }
    }

    if parent_station_ids.contains(stop_id) {
        return format!("parent::{stop_id}");
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

    let mut clusters: Vec<StopClusterDef> = Vec::new();
    let mut cluster_idx_by_key: HashMap<String, u32> = HashMap::new();
    let mut stop_idx_to_cluster_idx: Vec<u32> = vec![0; stops.len()];

    for (stop_idx, stop) in stops.iter().enumerate() {
        let cluster_key = stop_cluster_key(
            stop.id(),
            stop.name(),
            stop.parent_station(),
            &parent_station_ids,
        );

        let cluster_idx = if let Some(existing) = cluster_idx_by_key.get(&cluster_key).copied() {
            existing
        } else {
            let cluster_name = if let Some(parent_id) = cluster_key.strip_prefix("parent::") {
                stop_idx_by_id
                    .get(parent_id)
                    .and_then(|idx| stops.get(*idx as usize))
                    .map(|parent| parent.name().to_string())
                    .unwrap_or_else(|| stop.name().to_string())
            } else {
                stop.name().to_string()
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
        stop_idx_to_cluster_idx[stop_idx] = cluster_idx;
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
        stop_idx_to_cluster_idx,
    }
}
