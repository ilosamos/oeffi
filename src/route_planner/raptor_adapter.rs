use std::borrow::Cow;

use raptor::Timetable;

use super::model::{DEFAULT_TRANSFER_SECONDS, PlannerCache};

pub struct PlannerTimetable<'a> {
    pub cache: &'a PlannerCache,
    pub active_trips: Option<&'a [bool]>,
}

impl<'a> Timetable for PlannerTimetable<'a> {
    type Stop = u32;
    type Route = u32;
    type Trip = u32;

    fn get_routes_serving_stop(&self, stop: Self::Stop) -> Cow<'_, [Self::Route]> {
        Cow::Owned(
            self.cache
                .routes_serving_station
                .get(&stop)
                .cloned()
                .unwrap_or_default(),
        )
    }

    fn get_earlier_stop(
        &self,
        route: Self::Route,
        left: Self::Stop,
        right: Self::Stop,
    ) -> Self::Stop {
        let route_idx = route as usize;
        let left_pos = self.cache.route_station_pos[route_idx]
            .get(&left)
            .copied()
            .unwrap_or(usize::MAX);
        let right_pos = self.cache.route_station_pos[route_idx]
            .get(&right)
            .copied()
            .unwrap_or(usize::MAX);
        if left_pos <= right_pos { left } else { right }
    }

    fn get_stops_after(&self, route: Self::Route, stop: Self::Stop) -> Cow<'_, [Self::Stop]> {
        let route_idx = route as usize;
        let pos = self.cache.route_station_pos[route_idx]
            .get(&stop)
            .copied()
            .unwrap_or(0);
        Cow::Owned(self.cache.routes[route_idx].stations[pos..].to_vec())
    }

    fn get_earliest_trip(
        &self,
        route: Self::Route,
        at: raptor::Tau,
        stop: Self::Stop,
    ) -> Option<Self::Trip> {
        let route_idx = route as usize;
        let stop_pos = self.cache.route_station_pos[route_idx]
            .get(&stop)
            .copied()?;

        self.cache.trip_idxs_by_route[route_idx]
            .iter()
            .copied()
            .filter(|trip_idx| {
                if let Some(active_trips) = self.active_trips {
                    if !active_trips
                        .get(*trip_idx as usize)
                        .copied()
                        .unwrap_or(false)
                    {
                        return false;
                    }
                }
                let trip = &self.cache.trips[*trip_idx as usize];
                trip.times[stop_pos].1 >= at
            })
            .min_by_key(|trip_idx| {
                let trip = &self.cache.trips[*trip_idx as usize];
                trip.times[stop_pos].1
            })
    }

    fn get_arrival_time(&self, trip: Self::Trip, stop: Self::Stop) -> raptor::Tau {
        let trip_idx = trip as usize;
        let route_idx = self.cache.trips[trip_idx].route_idx as usize;
        let stop_pos = self.cache.route_station_pos[route_idx][&stop];
        self.cache.trips[trip_idx].times[stop_pos].0
    }

    fn get_departure_time(&self, trip: Self::Trip, stop: Self::Stop) -> raptor::Tau {
        let trip_idx = trip as usize;
        let route_idx = self.cache.trips[trip_idx].route_idx as usize;
        let stop_pos = self.cache.route_station_pos[route_idx][&stop];
        self.cache.trips[trip_idx].times[stop_pos].1
    }

    fn get_footpaths_from(&self, stop: Self::Stop) -> Cow<'_, [Self::Stop]> {
        Cow::Owned(self.cache.footpaths.get(&stop).cloned().unwrap_or_default())
    }

    fn get_transfer_time(&self, from: Self::Stop, to: Self::Stop) -> raptor::Tau {
        self.cache
            .transfer_times
            .get(&(from, to))
            .copied()
            .unwrap_or(DEFAULT_TRANSFER_SECONDS)
    }
}
